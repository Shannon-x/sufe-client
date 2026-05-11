package com.xboard.client.vm

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.xboard.client.AppContainer
import com.xboard.client.core.CheckoutResponse
import com.xboard.client.core.Client
import com.xboard.client.core.ConnectionManager
import com.xboard.client.core.ConnectionState
import com.xboard.client.core.FfiException
import com.xboard.client.core.ForgetPasswordArgs
import com.xboard.client.core.LoginArgs
import com.xboard.client.core.LoginSummary
import com.xboard.client.core.RegisterArgs
import com.xboard.client.core.SaveOrderArgs
import com.xboard.client.core.SaveTicketArgs
import com.xboard.client.core.StateObserver
import com.xboard.client.core.TunnelMode
import com.xboard.client.vpn.AndroidTunDelegate
import com.xboard.client.vpn.VpnBinder
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.concurrent.atomic.AtomicLong

/**
 * Single ViewModel for the whole app. Compose screens read what they
 * need via [authState] / [home] / [plans] / etc. and call action
 * methods on this VM.
 *
 * Why one VM rather than per-screen:
 *
 *   - The UniFFI [Client] is a single handle owning all auth + REST
 *     calls; spreading it across many VMs would mean each one re-doing
 *     the AppContainer.get() dance and racing on the same bearer.
 *
 *   - The connection state must outlive any individual screen — it's
 *     bound to a long-lived ConnectionManager + tokio runtime in Rust.
 *     A process-scoped VM keeps it alive across navigation.
 *
 *   - Mobile flows are simple enough that the convenience of one
 *     `LocalContext.current.appViewModel()` resolver outweighs the
 *     usual case for splitting.
 */
class AppViewModel(application: Application) : AndroidViewModel(application) {

    private val container: AppContainer = AppContainer.get(application)
    private val client: Client get() = container.client
    private var manager: ConnectionManager? = null
    private var tunDelegate: AndroidTunDelegate? = null
    private var trafficPollJob: Job? = null

    // ----- Auth -------------------------------------------------------------

    private val _authState = MutableStateFlow<AuthState>(AuthState.Idle)
    val authState: StateFlow<AuthState> = _authState.asStateFlow()

    // ----- Home / dashboard --------------------------------------------------

    private val _home = MutableStateFlow(HomeState())
    val home: StateFlow<HomeState> = _home.asStateFlow()

    // ----- Plans + payment ---------------------------------------------------

    private val _plans = MutableStateFlow(PlansState())
    val plans: StateFlow<PlansState> = _plans.asStateFlow()

    // ----- Orders ------------------------------------------------------------

    private val _orders = MutableStateFlow(OrdersState())
    val orders: StateFlow<OrdersState> = _orders.asStateFlow()

    // ----- Tickets -----------------------------------------------------------

    private val _tickets = MutableStateFlow(TicketsState())
    val tickets: StateFlow<TicketsState> = _tickets.asStateFlow()

    private val _ticketDetail = MutableStateFlow(TicketDetailState())
    val ticketDetail: StateFlow<TicketDetailState> = _ticketDetail.asStateFlow()

    // ----- Connection --------------------------------------------------------

    private val _connection = MutableStateFlow(ConnectionUiState())
    val connection: StateFlow<ConnectionUiState> = _connection.asStateFlow()

    // ----- One-shot events ---------------------------------------------------

    private val _events = MutableSharedFlow<UiEvent>(extraBufferCapacity = 8)
    val events: SharedFlow<UiEvent> = _events.asSharedFlow()
    private val eventCounter = AtomicLong()
    @Volatile
    private var autoConnectAfterAuth = false

    /** Hydrate persisted session on cold start. Call from MainActivity. */
    fun bootstrap() {
        if (_authState.value !is AuthState.Idle) return
        _authState.value = AuthState.Hydrating
        viewModelScope.launch {
            runCatching { client.hydrateSession() }
                .onSuccess { summary ->
                    if (summary != null) {
                        _authState.value = AuthState.Authenticated(summary)
                        refreshHome()
                    } else {
                        _authState.value = AuthState.Anonymous
                    }
                }
                .onFailure {
                    _authState.value = AuthState.Anonymous
                    snackbar(it.userMessage())
                }
        }
        loadSiteConfig()
    }

    private fun loadSiteConfig() {
        viewModelScope.launch {
            runCatching { client.fetchSiteConfig() }
                .onSuccess { sc -> _home.update { it.copy(siteConfig = sc) } }
                .onFailure { /* swallow on cold start; UI gracefully falls back */ }
        }
    }

    // ----- Auth actions ------------------------------------------------------

    fun login(email: String, password: String, recaptcha: String? = null, turnstile: String? = null) {
        if (_authState.value is AuthState.Submitting) return
        _authState.value = AuthState.Submitting
        viewModelScope.launch {
            runCatching {
                client.login(LoginArgs(email.trim(), password, recaptcha, turnstile))
            }.onSuccess {
                _authState.value = AuthState.Authenticated(it)
                autoConnectAfterAuth = true
                refreshHome()
            }.onFailure {
                _authState.value = AuthState.Anonymous
                snackbar(it.userMessage())
            }
        }
    }

    fun register(args: RegisterArgs) {
        if (_authState.value is AuthState.Submitting) return
        _authState.value = AuthState.Submitting
        viewModelScope.launch {
            runCatching { client.register(args) }
                .onSuccess {
                    _authState.value = AuthState.Authenticated(it)
                    autoConnectAfterAuth = true
                    refreshHome()
                }
                .onFailure {
                    _authState.value = AuthState.Anonymous
                    snackbar(it.userMessage())
                }
        }
    }

    fun forgetPassword(args: ForgetPasswordArgs, onDone: () -> Unit) {
        if (_authState.value is AuthState.Submitting) return
        _authState.value = AuthState.Submitting
        viewModelScope.launch {
            runCatching { client.forgetPassword(args) }
                .onSuccess {
                    _authState.value = AuthState.Anonymous
                    onDone()
                }
                .onFailure {
                    _authState.value = AuthState.Anonymous
                    snackbar(it.userMessage())
                }
        }
    }

    fun sendEmailCode(email: String, onDone: () -> Unit) {
        viewModelScope.launch {
            runCatching { client.sendEmailVerify(email.trim()) }
                .onSuccess { onDone() }
                .onFailure { snackbar(it.userMessage()) }
        }
    }

    fun logout() {
        viewModelScope.launch {
            disconnect()
            client.logout()
            _authState.value = AuthState.Anonymous
            _home.value = HomeState()
            _plans.value = PlansState()
            _orders.value = OrdersState()
            _tickets.value = TicketsState()
            _ticketDetail.value = TicketDetailState()
        }
    }

    // ----- Home / refresh actions -------------------------------------------

    fun refreshHome() {
        if (_authState.value !is AuthState.Authenticated) return
        _home.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            val user = runCatchingFfi { client.currentUser() }
            val sub = runCatchingFfi { client.currentSubscribe() }
            val notices = runCatchingFfi { client.fetchNotices() }
            _home.update {
                it.copy(
                    user = user ?: it.user,
                    subscribe = sub ?: it.subscribe,
                    notices = notices ?: it.notices,
                    refreshing = false,
                )
            }
        }
    }

    fun refreshNotices() {
        viewModelScope.launch {
            _home.update { it.copy(refreshing = true) }
            val n = runCatchingFfi { client.fetchNotices() }
            _home.update { it.copy(notices = n ?: it.notices, refreshing = false) }
        }
    }

    // ----- Plans actions ----------------------------------------------------

    fun refreshPlans() {
        _plans.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            val list = runCatchingFfi { client.fetchPlans() }
            val pms = runCatchingFfi { client.fetchPaymentMethods() }
            _plans.update {
                it.copy(
                    plans = list ?: it.plans,
                    paymentMethods = pms ?: it.paymentMethods,
                    refreshing = false,
                )
            }
        }
    }

    suspend fun saveOrder(args: SaveOrderArgs): String? = runCatchingFfi { client.saveOrder(args) }

    suspend fun checkoutOrder(tradeNo: String, methodId: Long): CheckoutResponse? =
        runCatchingFfi { client.checkoutOrder(tradeNo, methodId) }

    suspend fun checkOrder(tradeNo: String): Int? = runCatchingFfi { client.checkOrder(tradeNo) }

    suspend fun cancelOrder(tradeNo: String): Boolean {
        return runCatching { client.cancelOrder(tradeNo) }
            .onFailure { snackbar(it.userMessage()) }
            .isSuccess
    }

    // ----- Orders actions ---------------------------------------------------

    fun refreshOrders() {
        _orders.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            val list = runCatchingFfi { client.fetchOrders() }
            _orders.update { it.copy(orders = list ?: it.orders, refreshing = false) }
        }
    }

    // ----- Tickets actions --------------------------------------------------

    fun refreshTickets() {
        _tickets.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            val list = runCatchingFfi { client.fetchTickets() }
            _tickets.update { it.copy(tickets = list ?: it.tickets, refreshing = false) }
        }
    }

    fun openTicket(id: Long) {
        _ticketDetail.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            val detail = runCatchingFfi { client.fetchTicket(id) }
            _ticketDetail.update { it.copy(detail = detail, refreshing = false) }
        }
    }

    fun replyTicket(id: Long, message: String, onDone: () -> Unit) {
        viewModelScope.launch {
            runCatching { client.replyTicket(id, message) }
                .onSuccess {
                    openTicket(id)
                    onDone()
                }
                .onFailure { snackbar(it.userMessage()) }
        }
    }

    fun closeTicket(id: Long, onDone: () -> Unit) {
        viewModelScope.launch {
            runCatching { client.closeTicket(id) }
                .onSuccess {
                    openTicket(id)
                    refreshTickets()
                    onDone()
                }
                .onFailure { snackbar(it.userMessage()) }
        }
    }

    fun saveTicket(args: SaveTicketArgs, onDone: () -> Unit) {
        viewModelScope.launch {
            runCatching { client.saveTicket(args) }
                .onSuccess {
                    refreshTickets()
                    onDone()
                }
                .onFailure { snackbar(it.userMessage()) }
        }
    }

    // ----- Connection -------------------------------------------------------

    /**
     * Bind the VPN service (asks consent if needed) and call `connect`
     * on the kernel manager. Returns once the connect future resolves
     * — the foreground notification is up by then. Errors are routed
     * onto the snackbar / `connection.errorMessage`.
     */
    fun connect(binder: VpnBinder, announceAuto: Boolean = false) {
        viewModelScope.launch {
            if (announceAuto) {
                snackbar(getApplication<Application>().getString(
                    com.xboard.client.R.string.connect_auto_connecting,
                ))
            }
            _connection.update { it.copy(errorMessage = null) }
            val delegate: AndroidTunDelegate? = binder.prepareAndBind()
            if (delegate == null) {
                val msg = getApplication<Application>().getString(
                    com.xboard.client.R.string.connect_vpn_permission_denied,
                )
                _connection.update { it.copy(errorMessage = msg) }
                snackbar(msg)
                return@launch
            }
            ensureManager(delegate)
            runCatching { manager!!.connect() }
                .onSuccess {
                    if (announceAuto) {
                        snackbar(getApplication<Application>().getString(
                            com.xboard.client.R.string.connect_auto_connected,
                        ))
                    }
                }
                .onFailure {
                    val msg = it.userMessage()
                    _connection.update { it.copy(errorMessage = msg) }
                    snackbar(msg)
                }
            refreshProxies()
            startTrafficPolling()
        }
    }

    fun disconnect() {
        val mgr = manager ?: return
        viewModelScope.launch {
            stopTrafficPolling()
            runCatching { mgr.disconnect() }
                .onFailure { snackbar(it.userMessage()) }
        }
    }

    fun setMode(mode: TunnelMode) {
        val mgr = manager ?: return
        runCatching { mgr.setTunnelMode(mode) }
            .onSuccess {
                _connection.update { it.copy(mode = mode) }
            }
            .onFailure { snackbar(it.userMessage()) }
    }

    fun selectProxy(group: String, node: String) {
        val mgr = manager ?: return
        viewModelScope.launch {
            runCatching { mgr.selectProxy(group, node) }
                .onSuccess { refreshProxies() }
                .onFailure { snackbar(it.userMessage()) }
        }
    }

    suspend fun latencyTest(node: String): UInt? = runCatchingFfi {
        manager?.latencyTest(node)
    }

    fun refreshProxies() {
        val mgr = manager ?: return
        viewModelScope.launch {
            val list = runCatchingFfi { mgr.proxies() } ?: return@launch
            val primary = list.firstOrNull()
            val selected = primary?.now ?: list.firstOrNull { it.now != null }?.now
            val effective = selected?.let { resolveProxyLeaf(it, list) }
            _connection.update {
                it.copy(
                    proxies = list,
                    selectedNode = effective ?: selected,
                    selectedRoute = if (effective != null && effective != selected) {
                        listOfNotNull(primary?.name, selected).joinToString(" / ")
                    } else {
                        null
                    },
                )
            }
            if (_connection.value.state is ConnectionState.Connected) {
                tunDelegate?.reportNode(_connection.value.selectedNode)
            }
        }
    }

    fun consumeAutoConnectAfterAuth(): Boolean {
        if (!autoConnectAfterAuth) return false
        autoConnectAfterAuth = false
        return true
    }

    private fun ensureManager(delegate: AndroidTunDelegate) {
        if (manager != null) return
        tunDelegate = delegate
        val mgr = container.connectionManager(delegate)
        manager = mgr
        mgr.subscribeState(object : StateObserver {
            override fun onState(state: ConnectionState) {
                _connection.update { it.copy(state = state) }
                if (state is ConnectionState.Connected) {
                    delegate.reportNode(_connection.value.selectedNode)
                }
            }
        })
        _connection.update { it.copy(mode = mgr.requestedMode(), state = mgr.currentState()) }
    }

    private fun startTrafficPolling() {
        if (trafficPollJob?.isActive == true) return
        trafficPollJob = viewModelScope.launch {
            while (true) {
                val mgr = manager ?: break
                val t = runCatchingFfi { mgr.currentTraffic() } ?: break
                _connection.update { it.copy(traffic = t) }
                delay(1_000)
            }
        }
    }

    private fun stopTrafficPolling() {
        trafficPollJob?.cancel()
        trafficPollJob = null
        _connection.update { it.copy(traffic = null) }
    }

    // ----- Helpers ----------------------------------------------------------

    /**
     * Run a suspending FFI call, swallow exceptions onto the snackbar,
     * return null on failure. Use for "best-effort fill in this UI
     * field" — the field becomes `null` (= skeleton) on failure rather
     * than torpedoing the whole screen.
     */
    private suspend inline fun <T> runCatchingFfi(crossinline block: suspend () -> T): T? =
        withContext(Dispatchers.Default) {
            runCatching { block() }
                .onFailure { snackbar(it.userMessage()) }
                .getOrNull()
        }

    private fun snackbar(message: String) {
        val id = eventCounter.incrementAndGet()
        _events.tryEmit(UiEvent.Snackbar(id, message))
    }

    fun emitClipboardCopy(text: String, toastMessage: String) {
        val id = eventCounter.incrementAndGet()
        _events.tryEmit(UiEvent.CopyToClipboard(id, text, toastMessage))
    }

    private fun Throwable.userMessage(): String = when (this) {
        is FfiException -> message ?: this::class.java.simpleName
        else -> message ?: this::class.java.simpleName
    }

    private fun resolveProxyLeaf(name: String, groups: List<com.xboard.client.core.ProxyGroup>): String {
        val seen = LinkedHashSet<String>()
        var current = name
        while (seen.add(current)) {
            val group = groups.firstOrNull { it.name == current } ?: return current
            val next = group.now ?: return current
            current = next
        }
        return current
    }

    override fun onCleared() {
        super.onCleared()
        runCatching { manager?.unsubscribeState() }
        runCatching { manager?.close() }
        manager = null
    }
}
