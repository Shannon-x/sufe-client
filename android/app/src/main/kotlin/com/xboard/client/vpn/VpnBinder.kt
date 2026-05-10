package com.xboard.client.vpn

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.net.VpnService
import android.os.IBinder
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResult
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume

/**
 * Glue between the Compose UI, the system VPN-permission flow, and the
 * [XboardVpnService] binding. Owned by [com.xboard.client.MainActivity]
 * — registered in `onCreate` (so the ActivityResult contract is hot
 * before any composition wants to use [prepareAndBind]).
 *
 * Usage:
 *
 *   class MainActivity : ComponentActivity() {
 *       lateinit var vpnBinder: VpnBinder
 *       override fun onCreate(...) {
 *           super.onCreate(savedInstanceState)
 *           vpnBinder = VpnBinder(this)
 *           setContent { ... }
 *       }
 *       override fun onDestroy() {
 *           vpnBinder.unbind()
 *           super.onDestroy()
 *       }
 *   }
 *
 *   // Inside a coroutine:
 *   val delegate = vpnBinder.prepareAndBind() ?: return  // user denied
 */
class VpnBinder(private val activity: ComponentActivity) {

    private var pendingPermission: CompletableDeferred<Boolean>? = null
    private var pendingBind: CompletableDeferred<AndroidTunDelegate?>? = null
    private var currentDelegate: AndroidTunDelegate? = null

    private val permissionLauncher: ActivityResultLauncher<Intent> =
        activity.registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
                result: ActivityResult ->
            pendingPermission?.complete(result.resultCode == ComponentActivity.RESULT_OK)
            pendingPermission = null
        }

    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val local = service as? XboardVpnService.LocalBinder
            if (local == null) {
                pendingBind?.complete(null)
                pendingBind = null
                return
            }
            val delegate = AndroidTunDelegate(local)
            currentDelegate = delegate
            pendingBind?.complete(delegate)
            pendingBind = null
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            currentDelegate?.detach()
            currentDelegate = null
        }
    }

    /**
     * Resolve the user's VPN consent (if needed) then bind to the
     * service and return a delegate. Returns `null` if the user denied
     * consent or the service couldn't be bound.
     */
    suspend fun prepareAndBind(): AndroidTunDelegate? {
        currentDelegate?.let { return it }
        if (!ensurePermission()) return null
        return bindServiceSuspending()
    }

    private suspend fun ensurePermission(): Boolean {
        val intent = VpnService.prepare(activity) ?: return true
        val deferred = CompletableDeferred<Boolean>()
        pendingPermission = deferred
        permissionLauncher.launch(intent)
        return deferred.await()
    }

    private suspend fun bindServiceSuspending(): AndroidTunDelegate? =
        suspendCancellableCoroutine { cont ->
            val deferred = CompletableDeferred<AndroidTunDelegate?>()
            pendingBind = deferred
            val ok = activity.bindService(
                Intent(activity, XboardVpnService::class.java).apply {
                    action = XboardVpnService.ACTION_LOCAL_BIND
                },
                serviceConnection,
                Context.BIND_AUTO_CREATE,
            )
            if (!ok) {
                pendingBind = null
                cont.resume(null)
                return@suspendCancellableCoroutine
            }
            cont.invokeOnCancellation { runCatching { unbind() } }
            deferred.invokeOnCompletion {
                if (cont.isActive) cont.resume(deferred.getCompleted())
            }
        }

    /**
     * Tear down the bind. Safe to call repeatedly. Triggers
     * [XboardVpnService.onDestroy] when the last binding drops, which
     * in turn closes the tunnel.
     */
    fun unbind() {
        currentDelegate?.detach()
        currentDelegate = null
        runCatching { activity.unbindService(serviceConnection) }
    }
}
