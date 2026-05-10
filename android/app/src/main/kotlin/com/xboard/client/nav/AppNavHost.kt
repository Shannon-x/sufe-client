package com.xboard.client.nav

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.foundation.layout.Box
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.NavType
import androidx.navigation.navArgument
import com.xboard.client.ui.screens.ConnectScreen
import com.xboard.client.ui.screens.ForgetPasswordScreen
import com.xboard.client.ui.screens.HomeScreen
import com.xboard.client.ui.screens.LoginScreen
import com.xboard.client.ui.screens.NoticesScreen
import com.xboard.client.ui.screens.OrdersScreen
import com.xboard.client.ui.screens.PlansScreen
import com.xboard.client.ui.screens.RegisterScreen
import com.xboard.client.ui.screens.TicketDetailScreen
import com.xboard.client.ui.screens.TicketsScreen
import com.xboard.client.vm.AppViewModel
import com.xboard.client.vm.AuthState
import com.xboard.client.vm.UiEvent
import com.xboard.client.vpn.VpnBinder
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
import kotlinx.coroutines.flow.collectLatest

/**
 * Root navigation host. Top-level branches on [authState] — Compose
 * blows away the inner NavHost (and its back stack) when the user logs
 * in / out, which is exactly what we want.
 */
@Composable
fun AppNavHost(
    viewModel: AppViewModel,
    authState: AuthState,
    vpnBinder: VpnBinder,
) {
    val snackbarHostState = remember { SnackbarHostState() }
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current

    // One-shot UI events.
    LaunchedEffect(viewModel) {
        viewModel.events.collectLatest { event ->
            when (event) {
                is UiEvent.Snackbar -> snackbarHostState.showSnackbar(event.message)
                is UiEvent.CopyToClipboard -> {
                    clipboard.setText(AnnotatedString(event.text))
                    // Fall back to system clipboard manager for older versions
                    // that don't pick up the Compose clipboard write — both
                    // should be no-ops if the first one succeeded.
                    runCatching {
                        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                        cm?.setPrimaryClip(ClipData.newPlainText("xboard", event.text))
                    }
                    Toast.makeText(context, event.toast, Toast.LENGTH_SHORT).show()
                }
                is UiEvent.NavigateTo -> { /* reserved — not used yet */ }
            }
        }
    }

    Scaffold(
        modifier = Modifier.fillMaxSize(),
        snackbarHost = { SnackbarHost(snackbarHostState) },
    ) { padding ->
        Box(modifier = Modifier.padding(padding).fillMaxSize()) {
            when (authState) {
                AuthState.Idle, AuthState.Hydrating -> {
                    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator()
                    }
                }
                is AuthState.Anonymous, is AuthState.Submitting -> {
                    AuthFlow(viewModel = viewModel)
                }
                is AuthState.Authenticated -> {
                    MainFlow(viewModel = viewModel, vpnBinder = vpnBinder)
                }
            }
        }
    }
}

@Composable
private fun AuthFlow(viewModel: AppViewModel) {
    val nav = rememberNavController()
    NavHost(navController = nav, startDestination = Routes.LOGIN) {
        composable(Routes.LOGIN) {
            LoginScreen(
                viewModel = viewModel,
                onRegister = { nav.navigate(Routes.REGISTER) },
                onForget = { nav.navigate(Routes.FORGET) },
            )
        }
        composable(Routes.REGISTER) {
            RegisterScreen(
                viewModel = viewModel,
                onBack = { nav.popBackStack() },
            )
        }
        composable(Routes.FORGET) {
            ForgetPasswordScreen(
                viewModel = viewModel,
                onBack = { nav.popBackStack() },
            )
        }
    }
}

@Composable
private fun MainFlow(viewModel: AppViewModel, vpnBinder: VpnBinder) {
    val nav: NavHostController = rememberNavController()
    NavHost(navController = nav, startDestination = Routes.HOME) {
        composable(Routes.HOME) {
            HomeScreen(
                viewModel = viewModel,
                onOpenConnect = { nav.navigate(Routes.CONNECT) },
                onOpenPlans = { nav.navigate(Routes.PLANS) },
                onOpenOrders = { nav.navigate(Routes.ORDERS) },
                onOpenTickets = { nav.navigate(Routes.TICKETS) },
                onOpenNotices = { nav.navigate(Routes.NOTICES) },
            )
        }
        composable(Routes.CONNECT) {
            ConnectScreen(
                viewModel = viewModel,
                vpnBinder = vpnBinder,
                onBack = { nav.popBackStack() },
            )
        }
        composable(Routes.PLANS) {
            PlansScreen(viewModel = viewModel, onBack = { nav.popBackStack() })
        }
        composable(Routes.ORDERS) {
            OrdersScreen(viewModel = viewModel, onBack = { nav.popBackStack() })
        }
        composable(Routes.TICKETS) {
            TicketsScreen(
                viewModel = viewModel,
                onBack = { nav.popBackStack() },
                onOpen = { nav.navigate(Routes.ticketDetail(it)) },
            )
        }
        composable(
            Routes.TICKET_DETAIL,
            arguments = listOf(navArgument(Routes.TICKET_DETAIL_ARG) { type = NavType.LongType }),
        ) { entry ->
            val id = entry.arguments?.getLong(Routes.TICKET_DETAIL_ARG) ?: return@composable
            TicketDetailScreen(
                viewModel = viewModel,
                ticketId = id,
                onBack = { nav.popBackStack() },
            )
        }
        composable(Routes.NOTICES) {
            NoticesScreen(viewModel = viewModel, onBack = { nav.popBackStack() })
        }
    }
}
