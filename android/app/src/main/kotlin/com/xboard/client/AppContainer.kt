package com.xboard.client

import android.content.Context
import com.xboard.client.core.Client
import com.xboard.client.core.ConnectionManager
import com.xboard.client.core.TunDelegate
import com.xboard.client.secure.AndroidSecureStore
import com.xboard.client.vpn.AndroidTunDelegate
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/**
 * Process-singleton that owns the long-lived UniFFI handles.
 *
 * Two non-trivial bits worth knowing:
 *
 *   1. [Client] is constructed eagerly the first time the container is
 *      asked for it. The constructor is cheap (no I/O), but it does
 *      create the Keystore-backed master key the first time
 *      [AndroidSecureStore] is touched — that's why we resolve it off
 *      the main thread before the first composition.
 *
 *   2. [ConnectionManager] is *not* eagerly constructed. It depends on
 *      a [TunDelegate] supplied by the host (i.e. the bound
 *      VpnService), which only exists after the user grants VPN
 *      permission. Resolve it via [connectionManager] once the delegate
 *      is ready; calling it again with the same delegate is idempotent.
 *
 * Backend endpoints, encrypted discovery and feature policy come from the
 * shared deployment configuration. UI cannot supply another airport URL.
 */
class AppContainer private constructor(private val app: Context) {

    private val store: AndroidSecureStore by lazy { AndroidSecureStore(app) }

    private val locale: String by lazy {
        store.get(KEY_LOCALE) ?: app.resources.configuration.locales[0].toLanguageTag()
    }

    /** Lazily constructed; safe to call from any thread after [warmUp]. */
    val client: Client by lazy {
        Client.forDeployment(locale, store)
    }

    @Volatile
    private var managerInternal: ConnectionManager? = null
    private var delegateInternal: AndroidTunDelegate? = null
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    /**
     * Returns a [ConnectionManager] bound to [tunDelegate]. The first call
     * constructs and caches; subsequent calls return the same instance
     * regardless of the delegate argument — pass the same one or
     * [resetConnectionManager] first.
     */
    fun connectionManager(tunDelegate: TunDelegate?): ConnectionManager {
        synchronized(this) {
            if (tunDelegate is AndroidTunDelegate) {
                delegateInternal?.rebindFrom(tunDelegate)
            }
            managerInternal?.let { return it }
            delegateInternal = tunDelegate as? AndroidTunDelegate
            val kernelDir = app.applicationInfo.nativeLibraryDir
            val kernelPath = "$kernelDir/libmihomo.so"
            val workDir = app.filesDir.resolve("kernel").apply { mkdirs() }.absolutePath
            val cacheDir = app.cacheDir.resolve("kernel").apply { mkdirs() }.absolutePath
            return ConnectionManager(
                client,
                kernelPath,
                workDir,
                cacheDir,
                tunDelegate,
            ).also { managerInternal = it }
        }
    }

    /** Called when Android revokes or destroys the VPN service. */
    fun disconnectVpn() {
        val manager = managerInternal ?: return
        serviceScope.launch { runCatching { manager.disconnect() } }
    }

    /**
     * Drop the cached [ConnectionManager]. Use after the user grants /
     * revokes VPN permission so the next [connectionManager] call binds
     * to the new [TunDelegate].
     */
    fun resetConnectionManager() {
        synchronized(this) {
            managerInternal?.unsubscribeState()
            managerInternal?.close()
            managerInternal = null
        }
    }

    /**
     * Touches the SecureStore + Client off the main thread. Call from
     * [XboardApp.onCreate] inside `Dispatchers.IO` so the first
     * Composable doesn't block on Keystore provisioning.
     */
    fun warmUp() {
        // Force lazy resolution.
        @Suppress("UNUSED_EXPRESSION")
        client
    }

    companion object {
        private const val KEY_LOCALE = "xboard.locale"

        @Volatile
        private var instance: AppContainer? = null

        fun get(context: Context): AppContainer {
            instance?.let { return it }
            return synchronized(this) {
                instance ?: AppContainer(context.applicationContext).also { instance = it }
            }
        }
    }
}
