package com.xboard.client.vpn

import com.xboard.client.core.TunConfig
import com.xboard.client.core.TunDelegate
import com.xboard.client.core.TunnelException
import java.util.concurrent.atomic.AtomicReference

/**
 * Bridges the UniFFI [TunDelegate] callback interface to a bound
 * [XboardVpnService.LocalBinder]. Constructed by [VpnBinder] once the
 * service is bound; passed into [com.xboard.client.AppContainer.connectionManager].
 *
 * The Rust side calls [establishTun] from the kernel-manager tokio
 * runtime — the binder hop is in-process IPC and synchronous, so this
 * is safe.
 */
class AndroidTunDelegate(binder: XboardVpnService.LocalBinder) : TunDelegate {

    // AtomicReference so disconnect-then-rebind doesn't see a stale
    // service pointer if the UI re-binds quickly.
    private val binderRef = AtomicReference<XboardVpnService.LocalBinder?>(binder)

    override fun establishTun(config: TunConfig): Int {
        val binder = binderRef.get()
            ?: throw TunnelException.Backend("VPN service binder gone — UI must re-prepare()")
        return binder.establishTun(config)
    }

    override fun closeTun() {
        binderRef.get()?.closeTun()
    }

    /** Update the foreground notification with the current node name. */
    fun reportNode(name: String?) {
        binderRef.get()?.updateNotification(name)
    }

    /** Called from [VpnBinder] when the service connection drops. */
    fun detach() {
        binderRef.set(null)
    }
}
