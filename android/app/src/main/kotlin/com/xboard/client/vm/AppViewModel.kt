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
import kotlinx.coroutines.CancellationException
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
import org.json.JSONObject

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
    private var connectJob: Job? = null
    @Volatile private var connectionEpoch = 0L
    private val authEpoch = AtomicLong()
    private var kernelCleanupJob: Job? = null
    private var homeRequest = 0L
    private var noticesRequest = 0L
    private var plansRequest = 0L
    private var ordersRequest = 0L
    private var ticketsRequest = 0L
    private var ticketRequest = 0L
    private var proxiesRequest = 0L
    private var siteRequest = 0L
    private fun currentSession(version: Long) = version == authEpoch.get() && _authState.value is AuthState.Authenticated
    val business = BusinessController(
        clientProvider = { client }, scope = viewModelScope,
        preferences = application.getSharedPreferences("sufe_ui", android.content.Context.MODE_PRIVATE),
        notify = { snackbar(it) }, refreshAccount = { refreshHome() }, onExpired = { expireSession() },
    )

    // ----- Auth -------------------------------------------------------------

    private val _authState = MutableStateFlow<AuthState>(AuthState.Idle)
    val authState: StateFlow<AuthState> = _authState.asStateFlow()
    private val _emailCodeSending = MutableStateFlow(false)
    val emailCodeSending = _emailCodeSending.asStateFlow()

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

    init {
        viewModelScope.launch {
            authState.collect { auth ->
                if (auth is AuthState.Authenticated) business.startSession(auth.summary.email)
                else business.stopSession()
            }
        }
    }

    /** Hydrate persisted session on cold start. Call from MainActivity. */
    fun bootstrap() {
        if (_authState.value !is AuthState.Idle) return
        val version = authEpoch.incrementAndGet()
        _authState.value = AuthState.Hydrating
        viewModelScope.launch {
            try {
                val summary = client.hydrateSession()
                if (version != authEpoch.get()) return@launch
                if (summary == null) { _authState.value = AuthState.Anonymous; return@launch }
                // An explicit rejection expires the session. A temporary
                // network failure does not invalidate a persisted bearer.
                val valid = try { client.checkLogin() }
                    catch (error: CancellationException) { throw error }
                    catch (error: FfiException.Unauthorized) { false }
                    catch (_: Exception) { null }
                if (version != authEpoch.get()) return@launch
                if (valid == false) { expireSession(); return@launch }
                _authState.value = AuthState.Authenticated(summary)
                refreshHome()
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                if (version == authEpoch.get()) {
                    _authState.value = AuthState.Anonymous
                    snackbar(error.userMessage())
                }
            }
        }
        loadSiteConfig()
    }

    fun loadSiteConfig() {
        val request = ++siteRequest
        viewModelScope.launch {
            runCatching { client.fetchSiteConfig() }
                .onSuccess { sc -> if (request == siteRequest) _home.update { it.copy(siteConfig = sc) } }
                .onFailure { if (it is CancellationException) throw it }
        }
    }

    // ----- Auth actions ------------------------------------------------------

    fun login(email: String, password: String, recaptcha: String? = null, turnstile: String? = null) {
        if (_authState.value is AuthState.Submitting) return
        val version = authEpoch.incrementAndGet()
        _authState.value = AuthState.Submitting
        val cleanup = kernelCleanupJob
        viewModelScope.launch {
            cleanup?.join()
            if (version != authEpoch.get()) return@launch
            runCatching {
                client.login(LoginArgs(email.trim(), password, recaptcha, turnstile))
            }.onSuccess {
                if (version != authEpoch.get()) return@onSuccess
                _authState.value = AuthState.Authenticated(it)
                autoConnectAfterAuth = true
                refreshHome()
            }.onFailure {
                if (it is CancellationException) throw it
                if (version != authEpoch.get()) return@onFailure
                _authState.value = AuthState.Anonymous
                snackbar(it.userMessage())
            }
        }
    }

    fun register(args: RegisterArgs) {
        if (_authState.value is AuthState.Submitting) return
        val version = authEpoch.incrementAndGet()
        _authState.value = AuthState.Submitting
        val cleanup = kernelCleanupJob
        viewModelScope.launch {
            cleanup?.join()
            if (version != authEpoch.get()) return@launch
            runCatching { client.register(args) }
                .onSuccess {
                    if (version != authEpoch.get()) return@onSuccess
                    _authState.value = AuthState.Authenticated(it)
                    autoConnectAfterAuth = true
                    refreshHome()
                }
                .onFailure {
                    if (it is CancellationException) throw it
                    if (version != authEpoch.get()) return@onFailure
                    _authState.value = AuthState.Anonymous
                    snackbar(it.userMessage())
                }
        }
    }

    fun forgetPassword(args: ForgetPasswordArgs, onDone: () -> Unit) {
        if (_authState.value is AuthState.Submitting) return
        val version = authEpoch.incrementAndGet()
        _authState.value = AuthState.Submitting
        val cleanup = kernelCleanupJob
        viewModelScope.launch {
            cleanup?.join()
            if (version != authEpoch.get()) return@launch
            runCatching { client.forgetPassword(args) }
                .onSuccess {
                    if (version != authEpoch.get()) return@onSuccess
                    _authState.value = AuthState.Anonymous
                    onDone()
                }
                .onFailure {
                    if (it is CancellationException) throw it
                    if (version != authEpoch.get()) return@onFailure
                    _authState.value = AuthState.Anonymous
                    snackbar(it.userMessage())
                }
        }
    }

    fun sendEmailCode(email: String, captchaToken: String? = null, onDone: () -> Unit) {
        if (_emailCodeSending.value) return
        val version = authEpoch.get()
        _emailCodeSending.value = true
        viewModelScope.launch {
            try {
                client.sendEmailVerify(email.trim(), captchaToken)
                if (version == authEpoch.get()) onDone()
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                if (version == authEpoch.get()) snackbar(error.userMessage())
            } finally { _emailCodeSending.value = false }
        }
    }

    fun logout() { clearSession() }

    private fun expireSession() {
        if (_authState.value is AuthState.Anonymous) return
        clearSession()
        snackbar("登录已过期，请重新登录")
    }

    private fun clearSession() {
        authEpoch.incrementAndGet()
        connectionEpoch++
        autoConnectAfterAuth = false
        stopTrafficPolling()
        business.stopSession()
        _authState.value = AuthState.Anonymous
        _home.value = HomeState(siteConfig = _home.value.siteConfig)
        _plans.value = PlansState()
        _orders.value = OrdersState()
        _tickets.value = TicketsState()
        _ticketDetail.value = TicketDetailState()
        _connection.value = ConnectionUiState(mode = _connection.value.mode)
        val outstandingConnect = connectJob
        val previousCleanup = kernelCleanupJob
        // New login/connect waits for this barrier. The old connect may be
        // fetching its subscription before entering the native kernel lock.
        kernelCleanupJob = viewModelScope.launch {
            previousCleanup?.join()
            client.logout()
            outstandingConnect?.join()
            runCatching { manager?.disconnect() }
                .onFailure { if (it is CancellationException) throw it; snackbar("连接清理未完成，请重试断开连接") }
        }
    }

    // ----- Home / refresh actions -------------------------------------------

    fun refreshHome() {
        if (_authState.value !is AuthState.Authenticated) return
        val version = authEpoch.get(); val request = ++homeRequest; val noticeVersion = noticesRequest
        _home.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            if (!currentSession(version) || request != homeRequest) return@launch
            val user = runCatchingFfi { client.currentUser() }
            if (!currentSession(version) || request != homeRequest) return@launch
            val sub = runCatchingFfi { client.currentSubscribe() }
            if (!currentSession(version) || request != homeRequest) return@launch
            val notices = if (business.state.value.enabled("notice")) runCatchingFfi { client.fetchNotices() } else emptyList()
            if (!currentSession(version) || request != homeRequest) return@launch
            _home.update {
                it.copy(
                    user = user ?: it.user,
                    subscribe = sub ?: it.subscribe,
                    notices = if (noticeVersion == noticesRequest) notices ?: it.notices else it.notices,
                    refreshing = false,
                )
            }
        }
    }

    fun refreshNotices() {
        if (!business.state.value.enabled("notice") || _authState.value !is AuthState.Authenticated) return
        val version = authEpoch.get(); val request = ++noticesRequest
        viewModelScope.launch {
            if (!currentSession(version) || request != noticesRequest) return@launch
            _home.update { it.copy(refreshing = true) }
            val n = runCatchingFfi { client.fetchNotices() }
            if (!currentSession(version) || request != noticesRequest || !business.state.value.enabled("notice")) return@launch
            _home.update { it.copy(notices = n ?: it.notices, refreshing = false) }
        }
    }

    // ----- Plans actions ----------------------------------------------------

    fun refreshPlans() {
        if (_authState.value !is AuthState.Authenticated) return
        val version = authEpoch.get(); val request = ++plansRequest
        _plans.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            if (!currentSession(version) || request != plansRequest) return@launch
            val list = runCatchingFfi { client.fetchPlans() }
            if (!currentSession(version) || request != plansRequest) return@launch
            val pms = runCatchingFfi { client.fetchPaymentMethods() }
            if (!currentSession(version) || request != plansRequest) return@launch
            _plans.update {
                it.copy(
                    plans = list ?: it.plans,
                    paymentMethods = pms ?: it.paymentMethods,
                    refreshing = false,
                )
            }
        }
    }

    suspend fun saveOrder(args: SaveOrderArgs): String? {
        if (!business.state.value.enabled("purchase")) { snackbar("订阅购买暂未开放"); return null }
        return runCatchingFfi { client.saveOrder(args) }
    }

    suspend fun checkoutOrder(tradeNo: String, methodId: Long): CheckoutResponse? =
        runCatchingFfi { client.checkoutOrder(tradeNo, methodId) }

    suspend fun orderDetails(tradeNo: String): JSONObject? = runCatchingFfi { JSONObject(client.fetchOrderJson(tradeNo)) }
    suspend fun paymentMethods() = runCatchingFfi { client.fetchPaymentMethods() }
    suspend fun checkCoupon(code: String, planId: Long, period: String): JSONObject? {
        if (!business.state.value.enabled("purchase")) return null
        return runCatchingFfi { JSONObject(client.checkCouponForPeriodJson(code, planId, period)) }
    }

    suspend fun checkOrder(tradeNo: String): Int? = runCatchingFfi { client.checkOrder(tradeNo) }

    suspend fun cancelOrder(tradeNo: String): Boolean = runCatchingFfi { client.cancelOrder(tradeNo); true } ?: false

    // ----- Orders actions ---------------------------------------------------

    fun refreshOrders() {
        if (_authState.value !is AuthState.Authenticated) return
        val version = authEpoch.get(); val request = ++ordersRequest
        _orders.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            if (!currentSession(version) || request != ordersRequest) return@launch
            val list = runCatchingFfi { client.fetchOrders() }
            if (!currentSession(version) || request != ordersRequest) return@launch
            _orders.update { it.copy(orders = list ?: it.orders, refreshing = false) }
        }
    }

    // ----- Tickets actions --------------------------------------------------

    fun refreshTickets() {
        if (!business.state.value.enabled("tickets") || _authState.value !is AuthState.Authenticated) return
        val version = authEpoch.get(); val request = ++ticketsRequest
        _tickets.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            if (!currentSession(version) || request != ticketsRequest) return@launch
            val list = runCatchingFfi { client.fetchTickets() }
            if (!currentSession(version) || request != ticketsRequest || !business.state.value.enabled("tickets")) return@launch
            _tickets.update { it.copy(tickets = list ?: it.tickets, refreshing = false) }
        }
    }

    fun openTicket(id: Long) {
        if (!business.state.value.enabled("tickets") || _authState.value !is AuthState.Authenticated) return
        val version = authEpoch.get(); val request = ++ticketRequest
        _ticketDetail.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            if (!currentSession(version) || request != ticketRequest) return@launch
            val detail = runCatchingFfi { client.fetchTicket(id) }
            if (!currentSession(version) || request != ticketRequest || !business.state.value.enabled("tickets")) return@launch
            _ticketDetail.update { it.copy(detail = detail, refreshing = false) }
        }
    }

    fun replyTicket(id: Long, message: String, onDone: () -> Unit) {
        if (!business.state.value.enabled("tickets")) return
        val version = authEpoch.get()
        viewModelScope.launch {
            if (!currentSession(version)) return@launch
            val completed = runCatchingFfi { client.replyTicket(id, message); true } == true
            if (!completed || !currentSession(version)) return@launch
            openTicket(id)
            onDone()
        }
    }

    fun closeTicket(id: Long, onDone: () -> Unit) {
        if (!business.state.value.enabled("tickets")) return
        val version = authEpoch.get()
        viewModelScope.launch {
            if (!currentSession(version)) return@launch
            val completed = runCatchingFfi { client.closeTicket(id); true } == true
            if (!completed || !currentSession(version)) return@launch
            openTicket(id)
            refreshTickets()
            onDone()
        }
    }

    fun saveTicket(args: SaveTicketArgs, onDone: () -> Unit) {
        if (!business.state.value.enabled("tickets")) return
        val version = authEpoch.get()
        viewModelScope.launch {
            if (!currentSession(version)) return@launch
            val completed = runCatchingFfi { client.saveTicket(args); true } == true
            if (!completed || !currentSession(version)) return@launch
            refreshTickets()
            onDone()
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
        if (_authState.value !is AuthState.Authenticated) return
        if (_home.value.user?.banned == true) { snackbar("账户已暂停使用，请联系客服"); return }
        val subscription = _home.value.subscribe
        if (subscription != null && (subscription.planId == null || subscription.transferEnable <= subscription.upload + subscription.download || (subscription.expiredAt != null && subscription.expiredAt!! <= System.currentTimeMillis() / 1000))) {
            snackbar("订阅已到期或流量已用完，请先管理订阅")
            return
        }
        if (connectJob?.isActive == true || _connection.value.state is ConnectionState.Connected) return
        val epoch = connectionEpoch
        val cleanup = kernelCleanupJob
        connectJob = viewModelScope.launch {
            cleanup?.join()
            if (epoch != connectionEpoch || _authState.value !is AuthState.Authenticated) return@launch
            if (announceAuto) {
                snackbar(getApplication<Application>().getString(
                    com.xboard.client.R.string.connect_auto_connecting,
                ))
            }
            _connection.update { it.copy(errorMessage = null) }
            val delegate: AndroidTunDelegate? = binder.prepareAndBind()
            if (epoch != connectionEpoch || _authState.value !is AuthState.Authenticated) return@launch
            if (delegate == null) {
                val msg = getApplication<Application>().getString(
                    com.xboard.client.R.string.connect_vpn_permission_denied,
                )
                _connection.update { it.copy(errorMessage = msg) }
                snackbar(msg)
                return@launch
            }
            runCatching {
                ensureManager(delegate)
                manager!!.connect()
            }
                .onSuccess {
                    if (epoch != connectionEpoch || _authState.value !is AuthState.Authenticated) return@onSuccess
                    if (announceAuto) {
                        snackbar(getApplication<Application>().getString(
                            com.xboard.client.R.string.connect_auto_connected,
                        ))
                    }
                }
                .onFailure {
                    if (it is CancellationException) throw it
                    if (epoch != connectionEpoch || _authState.value !is AuthState.Authenticated) return@onFailure
                    if (it is FfiException.Unauthorized) { expireSession(); return@onFailure }
                    val msg = it.userMessage()
                    _connection.update { it.copy(errorMessage = msg) }
                    snackbar(msg)
                }
            if (epoch != connectionEpoch || _authState.value !is AuthState.Authenticated) return@launch
            if (_connection.value.state is ConnectionState.Connected) {
                refreshProxies()
                startTrafficPolling()
            }
        }
    }

    fun disconnect() {
        connectionEpoch++
        stopTrafficPolling()
        val mgr = manager ?: return
        val outstandingConnect = connectJob
        val previousCleanup = kernelCleanupJob
        kernelCleanupJob = viewModelScope.launch {
            previousCleanup?.join()
            outstandingConnect?.join()
            runCatching { mgr.disconnect() }
                .onFailure { if (it is CancellationException) throw it; snackbar(it.userMessage()) }
        }
    }

    fun setMode(mode: TunnelMode) {
        if (_authState.value !is AuthState.Authenticated) return
        val mgr = manager ?: return
        runCatching { mgr.setTunnelMode(mode) }
            .onSuccess {
                _connection.update { it.copy(mode = mode) }
            }
            .onFailure { snackbar(it.userMessage()) }
    }

    fun selectProxy(group: String, node: String) {
        if (_authState.value !is AuthState.Authenticated) return
        val version = authEpoch.get(); val connectionVersion = connectionEpoch
        val mgr = manager ?: return
        viewModelScope.launch {
            if (!currentSession(version)) return@launch
            runCatching { mgr.selectProxy(group, node) }
                .onSuccess { if (currentSession(version) && connectionVersion == connectionEpoch) refreshProxies() }
                .onFailure { snackbar(it.userMessage()) }
        }
    }

    suspend fun latencyTest(node: String): UInt? = runCatchingFfi {
        manager?.latencyTest(node)
    }

    fun refreshProxies() {
        if (_authState.value !is AuthState.Authenticated || _connection.value.state !is ConnectionState.Connected) return
        val version = authEpoch.get(); val connectionVersion = connectionEpoch; val request = ++proxiesRequest
        val mgr = manager ?: return
        viewModelScope.launch {
            if (!currentSession(version) || request != proxiesRequest) return@launch
            val list = runCatchingFfi { mgr.proxies() } ?: return@launch
            if (!currentSession(version) || connectionVersion != connectionEpoch || request != proxiesRequest || _connection.value.state !is ConnectionState.Connected) return@launch
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
        if (manager != null) {
            container.connectionManager(delegate)
            tunDelegate = delegate
            return
        }
        tunDelegate = delegate
        val mgr = container.connectionManager(delegate)
        manager = mgr
        mgr.subscribeState(object : StateObserver {
            override fun onState(state: ConnectionState) {
                val version = authEpoch.get()
                viewModelScope.launch(Dispatchers.Main.immediate) {
                    if (!currentSession(version)) return@launch
                    _connection.update { if (state is ConnectionState.Connected) it.copy(state = state) else it.copy(state = state, traffic = null, proxies = emptyList(), selectedNode = null, selectedRoute = null) }
                    if (state is ConnectionState.Connected) {
                        delegate.reportNode(_connection.value.selectedNode)
                        refreshProxies()
                        startTrafficPolling()
                    } else stopTrafficPolling()
                }
            }
        })
        _connection.update { it.copy(mode = mgr.requestedMode(), state = mgr.currentState()) }
    }

    private fun startTrafficPolling() {
        if (trafficPollJob?.isActive == true) return
        val version = authEpoch.get(); val connectionVersion = connectionEpoch
        trafficPollJob = viewModelScope.launch {
            while (currentSession(version) && connectionVersion == connectionEpoch && _connection.value.state is ConnectionState.Connected) {
                val mgr = manager ?: break
                val t = runCatchingFfi { mgr.currentTraffic() } ?: break
                if (!currentSession(version) || connectionVersion != connectionEpoch || _connection.value.state !is ConnectionState.Connected) break
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
    private suspend inline fun <T> runCatchingFfi(crossinline block: suspend () -> T): T? {
        val version = authEpoch.get()
        if (!currentSession(version)) return null
        return withContext(Dispatchers.Default) {
            if (!currentSession(version)) return@withContext null
            try {
                val result = block()
                if (version == authEpoch.get()) result else null
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                withContext(Dispatchers.Main.immediate) {
                    if (version == authEpoch.get()) {
                        if (error is FfiException.Unauthorized) expireSession()
                        else snackbar(error.userMessage())
                    }
                }
                null
            }
        }
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
        // AppContainer + the foreground service own the VPN across Activity
        // recreation. Closing the UniFFI handle here invalidated that cache.
        manager = null
    }
}
