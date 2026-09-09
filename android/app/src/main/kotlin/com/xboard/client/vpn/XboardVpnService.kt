package com.xboard.client.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Binder
import android.os.Build
import android.os.IBinder
import android.os.ParcelFileDescriptor
import android.util.Log
import androidx.core.app.NotificationCompat
import com.xboard.client.MainActivity
import com.xboard.client.AppContainer
import com.xboard.client.R
import com.xboard.client.core.TunConfig
import com.xboard.client.core.TunnelException

/**
 * The Android side of the TUN tunnel.
 *
 * Lifecycle:
 *
 *   1. UI calls [VpnService.prepare] and, on RESULT_OK, binds to this
 *      service. The bind hands back a [LocalBinder] that the
 *      [AndroidTunDelegate] adapter wraps.
 *
 *   2. Rust calls into the delegate's `establishTun` (UniFFI callback
 *      interface) → routes here via [LocalBinder.establishTun]. We
 *      build a [VpnService.Builder] from the [TunConfig], call
 *      `establish()` and lends its fd to Rust. The launcher preserves
 *      it across exec in the child; this service owns the parent copy.
 *
 *   3. We promote ourselves to a foreground service the moment the fd
 *      is established — Android kills VPN sessions if the host process
 *      goes background.
 *
 *   4. Disconnect path: Rust calls `closeTun` → we close the
 *      [ParcelFileDescriptor] and stopForeground/stopSelf.
 */
class XboardVpnService : VpnService() {

    inner class LocalBinder : Binder() {
        /** Caller is the UniFFI [AndroidTunDelegate] running on the
         *  Rust manager's tokio runtime — must not block. */
        @Throws(TunnelException::class)
        fun establishTun(config: TunConfig): Int = this@XboardVpnService.establishTunInternal(config)

        fun closeTun() = this@XboardVpnService.closeTunInternal()

        /** Update the foreground notification text. Safe to call from
         *  any thread; we hop to the main looper inside. */
        fun updateNotification(nodeName: String?) =
            this@XboardVpnService.updateNotificationText(nodeName)
    }

    private val binder = LocalBinder()
    @Volatile private var tunFd: ParcelFileDescriptor? = null
    private val networks = linkedSetOf<Network>()
    private var networkCallback: ConnectivityManager.NetworkCallback? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Started + foreground lifetime survives Activity recreation.
        if (tunFd != null) startForegroundCompat() else stopSelf(startId)
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder {
        // VpnService.onBind for the framework's `android.net.VpnService`
        // action must defer to super, which returns an internal Binder.
        // Our app-level bind uses a different action so we can hand
        // back the LocalBinder.
        if (intent?.action == ACTION_LOCAL_BIND) {
            return binder
        }
        return super.onBind(intent) ?: binder
    }

    override fun onRevoke() {
        Log.i(TAG, "VPN permission revoked by user / system")
        closeTunInternal()
        AppContainer.get(this).disconnectVpn()
        super.onRevoke()
    }

    override fun onDestroy() {
        closeTunInternal()
        AppContainer.get(this).disconnectVpn()
        super.onDestroy()
    }

    @Synchronized
    private fun establishTunInternal(config: TunConfig): Int {
        // Re-establish: tear down any prior fd before building a new one.
        tunFd?.close()
        tunFd = null

        val builder = Builder()
            .setSession(config.session)
            .addAddress(config.ipv4Addr, config.ipv4Prefix.toInt())
            .addAddress("fdfe:dcba:9876::1", 126)
            .addRoute("::", 0)
            .setMtu(config.mtu.toInt())
            // Allow the app's own traffic to bypass the tunnel — without
            // this, the kernel itself could deadlock trying to talk back
            // to the panel API through its own tunnel.
            .also { b ->
                b.addDisallowedApplication(packageName)
            }

        config.routes.ifEmpty { listOf("0.0.0.0/0") }.forEach { route ->
            val (addr, prefix) = parseCidr(route)
            builder.addRoute(addr, prefix)
        }
        config.dns.forEach { builder.addDnsServer(it) }

        val pfd = builder.establish()
            ?: throw TunnelException.Denied("VpnService.Builder.establish() returned null — VPN permission revoked or another VPN is active")

        tunFd = pfd

        // Promote to foreground the moment the tunnel is up.
        try {
            startService(Intent(this, XboardVpnService::class.java))
            startForegroundCompat()
            observeUnderlyingNetworks()
        } catch (error: Exception) {
            closeTunInternal()
            throw TunnelException.Backend("无法启动 VPN 前台服务：${error.message}")
        }
        return pfd.fd
    }

    @Synchronized
    private fun closeTunInternal() {
        tunFd?.let {
            runCatching { it.close() }
        }
        tunFd = null
        networkCallback?.let { callback ->
            val connectivity = getSystemService(ConnectivityManager::class.java)
            runCatching { connectivity.unregisterNetworkCallback(callback) }
        }
        networkCallback = null
        synchronized(networks) { networks.clear() }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            stopForeground(STOP_FOREGROUND_REMOVE)
        } else {
            @Suppress("DEPRECATION")
            stopForeground(true)
        }
        stopSelf()
    }

    private fun observeUnderlyingNetworks() {
        if (networkCallback != null) return
        val connectivity = getSystemService(ConnectivityManager::class.java)
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                synchronized(networks) {
                    networks.add(network)
                    setUnderlyingNetworks(networks.toTypedArray())
                }
            }

            override fun onLost(network: Network) {
                synchronized(networks) {
                    networks.remove(network)
                    setUnderlyingNetworks(networks.toTypedArray())
                }
            }
        }
        connectivity.registerNetworkCallback(
            NetworkRequest.Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
                .build(),
            callback,
        )
        networkCallback = callback
    }

    private fun startForegroundCompat() {
        ensureChannel()
        val notif = buildNotification(nodeName = null)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(
                NOTIFICATION_ID,
                notif,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE,
            )
        } else {
            startForeground(NOTIFICATION_ID, notif)
        }
    }

    private fun updateNotificationText(nodeName: String?) {
        if (tunFd == null) return  // not in foreground, skip
        val mgr = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        mgr.notify(NOTIFICATION_ID, buildNotification(nodeName))
    }

    private fun buildNotification(nodeName: String?): Notification {
        val openPi = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            },
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val text = if (nodeName != null) {
            getString(R.string.connect_vpn_notification_text, nodeName)
        } else {
            getString(R.string.connect_vpn_notification_title)
        }
        return NotificationCompat.Builder(this, NOTIFICATION_CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_notification_vpn)
            .setContentTitle(getString(R.string.connect_vpn_notification_title))
            .setContentText(text)
            .setOngoing(true)
            .setContentIntent(openPi)
            .setOnlyAlertOnce(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setVisibility(NotificationCompat.VISIBILITY_SECRET)
            .build()
    }

    private fun ensureChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val mgr = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (mgr.getNotificationChannel(NOTIFICATION_CHANNEL_ID) != null) return
        val channel = NotificationChannel(
            NOTIFICATION_CHANNEL_ID,
            getString(R.string.connect_vpn_notification_channel),
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = getString(R.string.connect_vpn_notification_channel)
            setShowBadge(false)
            enableLights(false)
            enableVibration(false)
        }
        mgr.createNotificationChannel(channel)
    }

    private fun parseCidr(cidr: String): Pair<String, Int> {
        val slash = cidr.indexOf('/')
        return if (slash >= 0) {
            cidr.substring(0, slash) to cidr.substring(slash + 1).toInt()
        } else {
            cidr to 32
        }
    }

    companion object {
        private const val TAG = "XboardVpnService"
        const val ACTION_LOCAL_BIND = "com.xboard.client.vpn.ACTION_LOCAL_BIND"
        private const val NOTIFICATION_ID = 4711
        private const val NOTIFICATION_CHANNEL_ID = "xboard_vpn"
    }
}
