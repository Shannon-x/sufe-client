import Foundation
import Observation
import UIKit

/// Single source of truth for the iOS shell. Held by the `App` scene and
/// passed into every screen via `@Bindable` / direct binding. Mirrors
/// `AppViewModel` on the Android side — but uses `@Observable` instead
/// of `StateFlow`, since SwiftUI watches mutated properties directly.
@Observable
@MainActor
final class AppModel {
    static let shared = AppModel()

    // ---------- session ----------
    var session: LoginSummary?
    var loginError: String?
    var isAuthBusy = false

    // ---------- home / subscribe ----------
    var siteConfig: SiteConfig?
    var user: UserInfo?
    var subscribe: SubscribeInfo?
    var notices: [Notice]?
    var homeRefreshing = false

    // ---------- plans / orders / tickets ----------
    var plans: [Plan]?
    var paymentMethods: [PaymentMethod]?
    var orders: [Order]?
    var tickets: [Ticket]?
    var ticketDetail: TicketDetail?
    var listsRefreshing = false

    // ---------- connection ----------
    var connectionState: ConnectionState = .disconnected
    var requestedMode: TunnelMode = .tun
    var proxies: [ProxyGroup] = []
    var selectedNode: String?
    var traffic: TrafficStats?

    // ---------- transient banners ----------
    var snackbar: String?

    // ---------- core handles ----------
    private var client: Client?
    private var manager: ConnectionManager?
    private let store = KeychainSecureStore()
    private let connectionController = ConnectionController()

    private init() {}

    // ---------- bootstrap ----------

    /// Construct the FFI `Client` and try to hydrate a previous session.
    /// Idempotent — calling it twice is a no-op.
    func bootstrap() async {
        if client != nil { return }
        do {
            let backend = backendBaseURL()
            let locale = Locale.current.identifier
            let c = try Client(backendBaseUrl: backend, locale: locale, secure: store)
            self.client = c
            self.session = try await c.hydrateSession()
        } catch {
            // First launch / Keychain empty → stay on Login screen.
            self.session = nil
        }
    }

    private func backendBaseURL() -> String {
        if let v = Bundle.main.object(forInfoDictionaryKey: "XboardDefaultBackendURL") as? String {
            return v
        }
        return "https://your-xboard-panel.example.com"
    }

    // ---------- auth ----------

    func login(email: String, password: String) async {
        guard let c = client else { return }
        isAuthBusy = true
        loginError = nil
        defer { isAuthBusy = false }
        do {
            session = try await c.login(args: LoginArgs(
                email: email,
                password: password,
                recaptcha: nil,
                turnstile: nil
            ))
        } catch {
            loginError = friendly(error)
        }
    }

    func register(email: String, password: String, code: String, invite: String?) async {
        guard let c = client else { return }
        isAuthBusy = true
        loginError = nil
        defer { isAuthBusy = false }
        do {
            session = try await c.register(args: RegisterArgs(
                email: email,
                password: password,
                emailCode: code,
                inviteCode: invite,
                recaptcha: nil,
                turnstile: nil
            ))
        } catch {
            loginError = friendly(error)
        }
    }

    func sendEmailCode(_ email: String) async {
        guard let c = client else { return }
        do { try await c.sendEmailVerify(email: email) }
        catch { snackbar = friendly(error) }
    }

    func forgetPassword(email: String, password: String, code: String) async {
        guard let c = client else { return }
        isAuthBusy = true
        defer { isAuthBusy = false }
        do {
            try await c.forgetPassword(args: ForgetPasswordArgs(
                email: email,
                password: password,
                emailCode: code,
                recaptcha: nil,
                turnstile: nil
            ))
            snackbar = String(localized: "auth.password_updated")
        } catch {
            loginError = friendly(error)
        }
    }

    func logout() async {
        guard let c = client else { return }
        await c.logout()
        session = nil
        user = nil
        subscribe = nil
        notices = nil
        plans = nil
        orders = nil
        tickets = nil
        ticketDetail = nil
    }

    // ---------- site config ----------

    func loadSiteConfig() async {
        guard let c = client, siteConfig == nil else { return }
        do { siteConfig = try await c.fetchSiteConfig() }
        catch { /* ignore — login form falls back to defaults */ }
    }

    // ---------- home ----------

    func refreshHome() async {
        guard let c = client else { return }
        homeRefreshing = true
        defer { homeRefreshing = false }
        async let u = c.currentUser()
        async let s = c.currentSubscribe()
        async let n = c.fetchNotices()
        do {
            user = try await u
            subscribe = try await s
            notices = try await n
        } catch {
            snackbar = friendly(error)
        }
    }

    func refreshNotices() async {
        guard let c = client else { return }
        do { notices = try await c.fetchNotices() }
        catch { snackbar = friendly(error) }
    }

    // ---------- plans / orders ----------

    func refreshPlans() async {
        guard let c = client else { return }
        listsRefreshing = true
        defer { listsRefreshing = false }
        do {
            plans = try await c.fetchPlans()
            paymentMethods = try await c.fetchPaymentMethods()
        } catch {
            snackbar = friendly(error)
        }
    }

    func refreshOrders() async {
        guard let c = client else { return }
        listsRefreshing = true
        defer { listsRefreshing = false }
        do { orders = try await c.fetchOrders() }
        catch { snackbar = friendly(error) }
    }

    func saveOrder(_ args: SaveOrderArgs) async throws -> String {
        guard let c = client else { throw AppError.notReady }
        return try await c.saveOrder(args: args)
    }

    func checkout(_ tradeNo: String, methodId: Int64) async throws -> CheckoutResponse {
        guard let c = client else { throw AppError.notReady }
        return try await c.checkoutOrder(tradeNo: tradeNo, methodId: methodId)
    }

    func checkOrderStatus(_ tradeNo: String) async throws -> Int32 {
        guard let c = client else { throw AppError.notReady }
        return try await c.checkOrder(tradeNo: tradeNo)
    }

    func cancelOrder(_ tradeNo: String) async {
        guard let c = client else { return }
        do { try await c.cancelOrder(tradeNo: tradeNo) }
        catch { snackbar = friendly(error) }
    }

    // ---------- tickets ----------

    func refreshTickets() async {
        guard let c = client else { return }
        listsRefreshing = true
        defer { listsRefreshing = false }
        do { tickets = try await c.fetchTickets() }
        catch { snackbar = friendly(error) }
    }

    func openTicket(id: Int64) async {
        guard let c = client else { return }
        do { ticketDetail = try await c.fetchTicket(id: id) }
        catch { snackbar = friendly(error) }
    }

    func replyTicket(id: Int64, message: String) async {
        guard let c = client else { return }
        do {
            try await c.replyTicket(id: id, message: message)
            await openTicket(id: id)
        } catch {
            snackbar = friendly(error)
        }
    }

    func closeTicket(id: Int64) async {
        guard let c = client else { return }
        do {
            try await c.closeTicket(id: id)
            await openTicket(id: id)
        } catch {
            snackbar = friendly(error)
        }
    }

    func saveTicket(_ args: SaveTicketArgs) async {
        guard let c = client else { return }
        do {
            _ = try await c.saveTicket(args: args)
            await refreshTickets()
        } catch {
            snackbar = friendly(error)
        }
    }

    // ---------- connection ----------

    /// Toggle the OS VPN state. iOS handles spawning the NE provider — we
    /// just ask `NETunnelProviderManager` to start, with a freshly rendered
    /// sing-box config in the App Group.
    func connect() async {
        guard let s = session else { return }
        do {
            // The NE provider doesn't have FFI access — it can't call
            // `current_subscribe()` itself. The main app fetches the YAML
            // and writes the JSON-rendered config to UserDefaults the
            // extension can read via `suiteName`.
            try await connectionController.start(
                subscribeToken: s.subscribeToken,
                backend: backendBaseURL(),
                mode: requestedMode
            )
            connectionState = .connecting(stage: .spawning, mode: requestedMode)
        } catch {
            snackbar = friendly(error)
            connectionState = .failed(message: friendly(error), mode: requestedMode)
        }
    }

    func disconnect() async {
        await connectionController.stop()
        connectionState = .disconnected
    }

    func setMode(_ mode: TunnelMode) {
        requestedMode = mode
    }

    func refreshProxies() async {
        guard let m = manager else { return }
        do { proxies = try await m.proxies() }
        catch { snackbar = friendly(error) }
    }

    func selectProxy(group: String, node: String) async {
        guard let m = manager else { return }
        do { try await m.selectProxy(group: group, node: node) }
        catch { snackbar = friendly(error) }
    }

    func latencyTest(_ node: String) async -> UInt32 {
        guard let m = manager else { return UInt32.max }
        do { return try await m.latencyTest(node: node) }
        catch { return UInt32.max }
    }

    // ---------- error formatting ----------

    private func friendly(_ error: Error) -> String {
        if let f = error as? FfiError {
            return String(describing: f)
        }
        if let s = error as? StorageError {
            return String(describing: s)
        }
        return error.localizedDescription
    }
}

enum AppError: Error {
    case notReady
}
