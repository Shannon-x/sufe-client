import Foundation
import Observation
import UIKit

struct ProxyGroupSnapshot: Identifiable, Hashable {
    var id: String { name }
    let name: String
    let kind: String
    var now: String?
    let all: [String]
}

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
    var proxies: [ProxyGroupSnapshot] = []
    var selectedNode: String?
    var selectedRoute: String?
    var traffic: TrafficStats?

    // ---------- transient banners ----------
    var snackbar: String?

    // ---------- core handles ----------
    private var client: Client?
    private var manager: ConnectionManager?
    private let store = KeychainSecureStore()
    private let connectionController = ConnectionController()
    private var subscribeYaml: String?
    private var selectedOverrides: [String: String] = [:]
    private var autoConnectAfterAuth = false

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
        return "https://imitate.cnqq.de"
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
            autoConnectAfterAuth = true
            await afterAuth()
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
            autoConnectAfterAuth = true
            await afterAuth()
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
        await disconnect()
        await c.logout()
        session = nil
        user = nil
        subscribe = nil
        notices = nil
        plans = nil
        orders = nil
        tickets = nil
        ticketDetail = nil
        proxies = []
        selectedNode = nil
        selectedRoute = nil
        subscribeYaml = nil
        selectedOverrides = [:]
    }

    private func afterAuth() async {
        await refreshHome()
        if autoConnectAfterAuth {
            autoConnectAfterAuth = false
            snackbar = String(localized: "connect.status.auto_connecting")
            await connect()
        }
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
            connectionState = .connecting(stage: .fetching, mode: requestedMode)
            // The NE provider doesn't have FFI access — it can't call
            // `current_subscribe()` itself. The main app fetches the YAML
            // and writes the JSON-rendered config to UserDefaults the
            // extension can read via `suiteName`.
            let yaml = try await connectionController.fetchSubscribeYAML(
                subscribeToken: s.subscribeToken,
                backend: backendBaseURL()
            )
            subscribeYaml = yaml
            updateProxySnapshot(from: yaml)

            connectionState = .connecting(stage: .spawning, mode: requestedMode)
            try await connectionController.start(
                subscribeYaml: applySelectionOverrides(to: yaml),
                mode: requestedMode
            )
            connectionState = .connected(since: Date(), mode: requestedMode, mixedPort: 7890)
            snackbar = String(localized: "connect.status.auto_connected")
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
        if let yaml = subscribeYaml {
            updateProxySnapshot(from: yaml)
            return
        }
        guard let s = session else { return }
        do {
            let yaml = try await connectionController.fetchSubscribeYAML(
                subscribeToken: s.subscribeToken,
                backend: backendBaseURL()
            )
            subscribeYaml = yaml
            updateProxySnapshot(from: yaml)
        } catch {
            snackbar = friendly(error)
        }
    }

    func selectProxy(group: String, node: String) async {
        selectedOverrides[group] = node
        updateSelectedNode(primary: group, current: node)
        if case .connected = connectionState {
            await connectionController.stop()
            await connect()
        }
    }

    func latencyTest(_ node: String) async -> UInt32 {
        guard let m = manager else { return UInt32.max }
        do { return try await m.latencyTest(node: node) }
        catch { return UInt32.max }
    }

    private func updateProxySnapshot(from yaml: String) {
        proxies = parseProxyGroups(from: yaml).map { group in
            var copy = group
            copy.now = selectedOverrides[group.name] ?? group.now
            return copy
        }
        if let primary = proxies.first {
            updateSelectedNode(primary: primary.name, current: primary.now)
        } else {
            selectedNode = nil
            selectedRoute = nil
        }
    }

    private func updateSelectedNode(primary: String?, current: String?) {
        guard let current else {
            selectedNode = nil
            selectedRoute = nil
            return
        }
        let effective = resolveProxyLeaf(current, groups: proxies)
        selectedNode = effective
        selectedRoute = effective == current ? nil : [primary, current].compactMap { $0 }.joined(separator: " / ")
    }

    private func resolveProxyLeaf(_ name: String, groups: [ProxyGroupSnapshot]) -> String {
        var seen = Set<String>()
        var current = name
        while !seen.contains(current) {
            seen.insert(current)
            guard let group = groups.first(where: { $0.name == current }), let next = group.now else {
                return current
            }
            current = next
        }
        return current
    }

    private func parseProxyGroups(from yaml: String) -> [ProxyGroupSnapshot] {
        let lines = yaml.components(separatedBy: .newlines)
        var blocks: [[String]] = []
        var current: [String] = []
        var inGroups = false

        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed == "proxy-groups:" {
                inGroups = true
                continue
            }
            if inGroups && !line.hasPrefix(" ") && !line.hasPrefix("-") && !trimmed.isEmpty {
                break
            }
            guard inGroups else { continue }
            if trimmed.hasPrefix("- ") {
                if !current.isEmpty { blocks.append(current) }
                current = [String(trimmed.dropFirst(2))]
            } else if !current.isEmpty {
                current.append(trimmed)
            }
        }
        if !current.isEmpty { blocks.append(current) }

        return blocks.compactMap { block in
            let joined = block.joined(separator: "\n")
            guard let name = yamlScalar("name", in: joined) else { return nil }
            let kind = normalizeGroupKind(yamlScalar("type", in: joined) ?? "select")
            let nodes = yamlList("proxies", in: block)
            guard !nodes.isEmpty else { return nil }
            return ProxyGroupSnapshot(name: name, kind: kind, now: nodes.first, all: nodes)
        }
    }

    private func yamlScalar(_ key: String, in text: String) -> String? {
        for part in text.replacingOccurrences(of: "{", with: "\n")
            .replacingOccurrences(of: "}", with: "\n")
            .components(separatedBy: CharacterSet(charactersIn: ",\n")) {
            let trimmed = part.trimmingCharacters(in: .whitespaces)
            guard trimmed.hasPrefix("\(key):") else { continue }
            return cleanYamlToken(String(trimmed.dropFirst(key.count + 1)))
        }
        return nil
    }

    private func yamlList(_ key: String, in block: [String]) -> [String] {
        let joined = block.joined(separator: "\n")
        if let start = joined.range(of: "\(key): ["),
           let end = joined[start.upperBound...].firstIndex(of: "]") {
            return joined[start.upperBound..<end]
                .split(separator: ",")
                .map { cleanYamlToken(String($0)) }
                .filter { !$0.isEmpty }
        }

        guard let idx = block.firstIndex(where: { $0 == "\(key):" }) else { return [] }
        return block[(idx + 1)...]
            .prefix { $0.hasPrefix("- ") }
            .map { cleanYamlToken(String($0.dropFirst(2))) }
            .filter { !$0.isEmpty }
    }

    private func cleanYamlToken(_ value: String) -> String {
        value.trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "\"'"))
    }

    private func normalizeGroupKind(_ raw: String) -> String {
        switch raw.lowercased() {
        case "select", "selector": return "Selector"
        case "url-test", "urltest": return "URLTest"
        case "fallback": return "Fallback"
        case "load-balance", "loadbalance": return "LoadBalance"
        default: return raw
        }
    }

    private func applySelectionOverrides(to yaml: String) -> String {
        var patched = yaml
        for (group, selected) in selectedOverrides {
            patched = moveNodeToFront(group: group, node: selected, yaml: patched)
        }
        return patched
    }

    private func moveNodeToFront(group: String, node: String, yaml: String) -> String {
        let lines = yaml.components(separatedBy: .newlines)
        var out: [String] = []
        var pendingGroup: String?
        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("- ") {
                pendingGroup = yamlScalar("name", in: String(trimmed.dropFirst(2))) ?? pendingGroup
            }
            if pendingGroup == group,
               let range = line.range(of: "proxies: ["),
               let end = line[range.upperBound...].firstIndex(of: "]") {
                let before = line[..<range.upperBound]
                let after = line[end...]
                var nodes = line[range.upperBound..<end]
                    .split(separator: ",")
                    .map { cleanYamlToken(String($0)) }
                    .filter { !$0.isEmpty }
                if let idx = nodes.firstIndex(of: node) {
                    nodes.remove(at: idx)
                    nodes.insert(node, at: 0)
                    out.append("\(before)\(nodes.joined(separator: ", "))\(after)")
                    continue
                }
            }
            out.append(line)
        }
        return out.joined(separator: "\n")
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
