package com.xboard.client

import android.app.Application
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/**
 * Process Application. Two responsibilities:
 *
 *   1. Provision the [AppContainer] singleton so the first composable
 *      to ask for it doesn't pay for Keystore master-key creation.
 *
 *   2. Hold a process-scoped [applicationScope] for any best-effort
 *      housekeeping that should outlive a single Activity (e.g. cache
 *      cleanup) — currently unused; kept here so future code has a
 *      well-defined home rather than spawning ad-hoc GlobalScope tasks.
 */
class XboardApp : Application() {

    val applicationScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override fun onCreate() {
        super.onCreate()
        applicationScope.launch {
            AppContainer.get(this@XboardApp).warmUp()
        }
    }
}
