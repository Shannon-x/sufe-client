package com.xboard.client

import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xboard.client.nav.AppNavHost
import com.xboard.client.ui.theme.XboardTheme
import com.xboard.client.vm.AppViewModel
import com.xboard.client.vpn.VpnBinder

class MainActivity : ComponentActivity() {

    /**
     * Lazily constructed in onCreate so the ActivityResult contract
     * registered inside [VpnBinder]'s `init` runs while the activity
     * is in CREATED — required by the AndroidX activity-result API.
     */
    lateinit var vpnBinder: VpnBinder
        private set

    private val notificationPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { /* result is not blocking — VPN keeps working without notifications */ }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        vpnBinder = VpnBinder(this)
        maybeRequestNotificationPermission()

        setContent {
            XboardTheme {
                val vm: AppViewModel = viewModel()
                val authState by vm.authState.collectAsState()
                LaunchedEffect(Unit) { vm.bootstrap() }
                AppNavHost(
                    viewModel = vm,
                    authState = authState,
                    vpnBinder = vpnBinder,
                )
            }
        }
    }

    private fun maybeRequestNotificationPermission() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
        // On Tiramisu+, POST_NOTIFICATIONS is a runtime permission. The
        // VPN foreground notification still works without it (system
        // categories are exempt from the user's "block notifications"
        // toggle in some launchers), but we ask anyway so the
        // node-name updates show through.
        notificationPermissionLauncher.launch(android.Manifest.permission.POST_NOTIFICATIONS)
    }

    override fun onDestroy() {
        if (::vpnBinder.isInitialized) vpnBinder.unbind()
        super.onDestroy()
    }
}
