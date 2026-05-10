package com.xboard.client.vm

import com.xboard.client.core.ConnectionState
import com.xboard.client.core.LoginSummary
import com.xboard.client.core.Notice
import com.xboard.client.core.Order
import com.xboard.client.core.PaymentMethod
import com.xboard.client.core.Plan
import com.xboard.client.core.ProxyGroup
import com.xboard.client.core.SiteConfig
import com.xboard.client.core.SubscribeInfo
import com.xboard.client.core.Ticket
import com.xboard.client.core.TicketDetail
import com.xboard.client.core.TrafficStats
import com.xboard.client.core.TunnelMode
import com.xboard.client.core.UserInfo

/**
 * Auth flow state machine. Owned by [AppViewModel.authState].
 *
 * Transitions:
 *   Idle → Hydrating (on app start) → Authenticated | Anonymous
 *   Anonymous → Submitting (on login/register) → Authenticated | Anonymous
 *   Authenticated → Anonymous (on logout / 401)
 */
sealed interface AuthState {
    /** App just started; haven't tried to hydrate the session yet. */
    data object Idle : AuthState

    /** Calling Client.hydrate_session(). */
    data object Hydrating : AuthState

    /** No saved session — show login screen. */
    data object Anonymous : AuthState

    /** Auth call in flight (login / register / forget). */
    data object Submitting : AuthState

    /** Session valid. Carries the LoginSummary the FFI returned. */
    data class Authenticated(val summary: LoginSummary) : AuthState
}

/**
 * Snapshot for the home dashboard. `null` fields mean "not loaded yet"
 * vs. empty-list which means "loaded, none". Allows the UI to render a
 * skeleton vs. an empty-state distinction.
 */
data class HomeState(
    val user: UserInfo? = null,
    val subscribe: SubscribeInfo? = null,
    val notices: List<Notice>? = null,
    val siteConfig: SiteConfig? = null,
    val refreshing: Boolean = false,
)

/** Plans + payment methods for the purchase flow. */
data class PlansState(
    val plans: List<Plan>? = null,
    val paymentMethods: List<PaymentMethod>? = null,
    val refreshing: Boolean = false,
)

/** Order list page. */
data class OrdersState(
    val orders: List<Order>? = null,
    val refreshing: Boolean = false,
)

/** Tickets index page. */
data class TicketsState(
    val tickets: List<Ticket>? = null,
    val refreshing: Boolean = false,
)

/** Single-ticket detail page; key is the ticket id. */
data class TicketDetailState(
    val detail: TicketDetail? = null,
    val refreshing: Boolean = false,
)

/**
 * Connection / traffic snapshot derived from
 * [ConnectionManager.subscribe_state] + [ConnectionManager.current_traffic].
 *
 * `selectedNode` is the human-readable name of the currently-selected
 * proxy in the primary group — pulled from [ProxyGroup.now] on the
 * "GLOBAL" or first selectable group.
 */
data class ConnectionUiState(
    val state: ConnectionState = ConnectionState.Disconnected,
    val mode: TunnelMode = TunnelMode.TUN,
    val proxies: List<ProxyGroup> = emptyList(),
    val selectedNode: String? = null,
    val traffic: TrafficStats? = null,
    val recentLog: String = "",
    val errorMessage: String? = null,
)

/**
 * One-shot UI events: snackbar messages, navigation triggers, etc.
 * Consumed via [androidx.compose.runtime.LaunchedEffect] keyed on the
 * event id.
 */
sealed interface UiEvent {
    val id: Long

    data class Snackbar(override val id: Long, val message: String) : UiEvent
    data class NavigateTo(override val id: Long, val route: String) : UiEvent
    data class CopyToClipboard(override val id: Long, val text: String, val toast: String) : UiEvent
}
