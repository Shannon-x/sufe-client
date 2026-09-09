package com.xboard.client.nav

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Language
import androidx.compose.material.icons.filled.ShoppingBag
import androidx.compose.material.icons.filled.SupportAgent
import androidx.compose.material.icons.filled.Person
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
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
import androidx.navigation.compose.currentBackStackEntryAsState
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
import com.xboard.client.ui.screens.AccountScreen
import com.xboard.client.ui.screens.RulesScreen
import com.xboard.client.ui.screens.FeaturesScreen
import com.xboard.client.ui.screens.SupportScreen
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
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_START) viewModel.business.setForeground(true)
            if (event == Lifecycle.Event.ON_STOP) viewModel.business.setForeground(false)
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

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
                    LaunchedEffect(authState.summary.subscribeToken) {
                        if (viewModel.consumeAutoConnectAfterAuth()) {
                            viewModel.connect(vpnBinder, announceAuto = true)
                        }
                    }
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
    val business by viewModel.business.state.collectAsState()
    val entry by nav.currentBackStackEntryAsState()
    val required = when (entry?.destination?.route) {
        Routes.PLANS -> "purchase"
        Routes.RULES -> "custom_rules"
        Routes.TICKETS, Routes.TICKET_DETAIL -> "tickets"
        Routes.NOTICES -> "notice"
        else -> null
    }
    LaunchedEffect(required, business.config, business.hidden) {
        if (required != null && !business.enabled(required)) nav.popBackStack(Routes.HOME, false)
    }
    Scaffold(bottomBar = {
        NavigationBar {
            val tabs = buildList {
                add(Triple(Routes.HOME, "总览", Icons.Default.Home))
                add(Triple(Routes.CONNECT, "节点", Icons.Default.Language))
                if (business.enabled("purchase")) add(Triple(Routes.PLANS, "订阅", Icons.Default.ShoppingBag))
                if (business.enabled("chatwoot")) add(Triple(Routes.SUPPORT, "客服", Icons.Default.SupportAgent))
                add(Triple(Routes.ACCOUNT, "我的", Icons.Default.Person))
            }
            val selected = when (entry?.destination?.route) {
                Routes.ORDERS, Routes.RULES, Routes.FEATURES -> Routes.ACCOUNT
                else -> entry?.destination?.route
            }
            tabs.forEach { (route, label, icon) -> NavigationBarItem(
                selected = selected == route,
                onClick = { nav.navigate(route) { popUpTo(Routes.HOME) { saveState = true }; launchSingleTop = true; restoreState = true } },
                icon = { Icon(icon, contentDescription = null) }, label = { Text(label) },
            ) }
        }
    }) { inner ->
    NavHost(navController = nav, startDestination = Routes.HOME, modifier = Modifier.padding(inner)) {
        composable(Routes.HOME) {
            HomeScreen(
                viewModel = viewModel,
                vpnBinder = vpnBinder,
                onOpenConnect = { nav.navigate(Routes.CONNECT) },
                onOpenPlans = { nav.navigate(Routes.PLANS) },
                onOpenOrders = { nav.navigate(Routes.ORDERS) },
                onOpenTickets = { nav.navigate(Routes.TICKETS) },
                onOpenNotices = { nav.navigate(Routes.NOTICES) },
                onOpenAccount = { nav.navigate(Routes.ACCOUNT) },
                onOpenRules = { nav.navigate(Routes.RULES) },
                onOpenFeatures = { nav.navigate(Routes.FEATURES) },
                onOpenSupport = { nav.navigate(Routes.SUPPORT) },
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
            if (business.enabled("purchase")) PlansScreen(viewModel = viewModel, onBack = { nav.popBackStack() })
        }
        composable(Routes.ORDERS) {
            OrdersScreen(viewModel = viewModel, onBack = { nav.popBackStack() })
        }
        composable(Routes.TICKETS) {
            if (business.enabled("tickets")) TicketsScreen(
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
            if (business.enabled("tickets")) TicketDetailScreen(
                viewModel = viewModel,
                ticketId = id,
                onBack = { nav.popBackStack() },
            )
        }
        composable(Routes.NOTICES) {
            if (business.enabled("notice")) NoticesScreen(viewModel = viewModel, onBack = { nav.popBackStack() })
        }
        composable(Routes.ACCOUNT) { AccountScreen(viewModel, onBack = { nav.popBackStack() }, onOrders = { nav.navigate(Routes.ORDERS) }, onRules = { nav.navigate(Routes.RULES) }, onFeatures = { nav.navigate(Routes.FEATURES) }) }
        composable(Routes.RULES) { if (business.enabled("custom_rules")) RulesScreen(viewModel, onBack = { nav.popBackStack() }) }
        composable(Routes.FEATURES) { FeaturesScreen(viewModel, onBack = { nav.popBackStack() }) }
        composable(Routes.SUPPORT) { SupportScreen(viewModel, onBack = { nav.popBackStack() }, onTickets = { nav.navigate(Routes.TICKETS) }) }
    }
    }
}
