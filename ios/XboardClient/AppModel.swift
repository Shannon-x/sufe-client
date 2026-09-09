import Foundation
import Observation
import UIKit
import NetworkExtension

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

    // Features and account state share the Rust deployment/transport layer.
    var clientFeatures = MobileClientConfig()
    var featuresLoaded = false
    var giftHistory: [GiftHistory.Entry] = []
    var inviteSummary: InviteSummary?
    var benefitsBusy = false
    var benefitsError: String?
    var customRules: [MobileRule] = []
    var rulesBusy = false
    var chatConversations: [ChatConversation] = []
    var chatMessages: [ChatMessage] = []
    var chatSelectedID: Int64?
    var chatBusy = false
    var chatSending = false
    var chatError: String?
    var chatViewing = false
    var chatUnread = 0
    var chatLastSeen: Int64 = 0
    var chatGeneration = 0
    var authGeneration = 0
    var featuresLastRefresh = Date.distantPast
    var featuresRefreshing = false

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
    private var connectionGeneration = 0
    private var trafficTask: Task<Void, Never>?

    private init() {
        selectedOverrides = (UserDefaults.standard.dictionary(forKey: "sufe.nodeSelections") as? [String: String]) ?? [:]
        connectionController.onStatusChange = { [weak self] status in
            guard let self else { return }
            switch status {
            case .connected:
                self.connectionState = .connected(since: ISO8601DateFormatter().string(from: Date()), mode: .tun, mixedPort: 7890)
                self.startTrafficPolling()
            case .connecting, .reasserting:
                self.connectionState = .connecting(stage: .spawning, mode: .tun)
            case .disconnected, .invalid:
                self.trafficTask?.cancel()
                self.trafficTask = nil
                self.traffic = nil
                if case .connected = self.connectionState { self.connectionState = .disconnected }
            default: break
            }
        }
    }

    // ---------- bootstrap ----------

    /// Construct the FFI `Client` and try to hydrate a previous session.
    /// Idempotent — calling it twice is a no-op.
    func bootstrap() async {
        if client != nil { return }
        isAuthBusy = true
        let generation = authGeneration
        defer { if generation == authGeneration { isAuthBusy = false } }
        do {
            let locale = Locale.current.identifier
            let c = try Client.forDeployment(locale: locale, secure: store)
            self.client = c
            await refreshClientFeatures()
            let hydrated = try await c.hydrateSession()
            guard generation == authGeneration else { return }
            self.session = hydrated
            if session != nil {
                do {
                    if !(try await c.checkLogin()) { await logout(); return }
                } catch { /* Keep a restorable session during temporary network failure. */ }
            }
            await connectionController.restoreStatus()
        } catch {
            // First launch / Keychain empty → stay on Login screen.
            self.session = nil
        }
    }

    // ---------- auth ----------

    func login(email: String, password: String, captchaToken: String? = nil) async {
        guard !isAuthBusy, let c = client else { return }
        isAuthBusy = true
        loginError = nil
        authGeneration += 1
        let generation = authGeneration
        defer { if generation == authGeneration { isAuthBusy = false } }
        do {
            let loggedIn = try await c.login(args: LoginArgs(
                email: email,
                password: password,
                recaptcha: siteConfig?.captchaType == "turnstile" ? nil : captchaToken,
                turnstile: siteConfig?.captchaType == "turnstile" ? captchaToken : nil
            ))
            guard generation == authGeneration else { return }
            session = loggedIn
            autoConnectAfterAuth = UserDefaults.standard.bool(forKey: "sufe.autoConnect")
            await afterAuth()
        } catch {
            if generation == authGeneration { loginError = friendly(error) }
        }
    }

    func register(email: String, password: String, code: String, invite: String?, captchaToken: String? = nil) async {
        guard !isAuthBusy, let c = client else { return }
        isAuthBusy = true
        loginError = nil
        authGeneration += 1
        let generation = authGeneration
        defer { if generation == authGeneration { isAuthBusy = false } }
        do {
            let registered = try await c.register(args: RegisterArgs(
                email: email,
                password: password,
                emailCode: code,
                inviteCode: invite,
                recaptcha: siteConfig?.captchaType == "turnstile" ? nil : captchaToken,
                turnstile: siteConfig?.captchaType == "turnstile" ? captchaToken : nil
            ))
            guard generation == authGeneration else { return }
            session = registered
            autoConnectAfterAuth = UserDefaults.standard.bool(forKey: "sufe.autoConnect")
            await afterAuth()
        } catch {
            if generation == authGeneration { loginError = friendly(error) }
        }
    }

    func sendEmailCode(_ email: String, captchaToken: String? = nil) async {
        guard !isAuthBusy, let c = client else { return }
        let generation = authGeneration
        do { try await c.sendEmailVerify(email: email, captchaToken: captchaToken) }
        catch { if generation == authGeneration { snackbar = friendly(error) } }
    }

    func forgetPassword(email: String, password: String, code: String, captchaToken: String? = nil) async {
        guard !isAuthBusy, let c = client else { return }
        isAuthBusy = true
        loginError = nil
        let generation = authGeneration
        defer { if generation == authGeneration { isAuthBusy = false } }
        do {
            try await c.forgetPassword(args: ForgetPasswordArgs(
                email: email,
                password: password,
                emailCode: code,
                recaptcha: siteConfig?.captchaType == "turnstile" ? nil : captchaToken,
                turnstile: siteConfig?.captchaType == "turnstile" ? captchaToken : nil
            ))
            guard generation == authGeneration else { return }
            snackbar = String(localized: "auth.password_updated")
        } catch {
            if generation == authGeneration { loginError = friendly(error) }
        }
    }

    func logout() async {
        guard session != nil, let c = client else { return }
        isAuthBusy = true
        defer { isAuthBusy = false }
        authGeneration += 1
        session = nil
        user = nil
        subscribe = nil
        notices = nil
        plans = nil
        orders = nil
        tickets = nil
        ticketDetail = nil
        paymentMethods = nil
        giftHistory = []
        inviteSummary = nil
        customRules = []
        resetChat()
        proxies = []
        selectedNode = nil
        selectedRoute = nil
        subscribeYaml = nil
        selectedOverrides = [:]
        UserDefaults.standard.removeObject(forKey: "sufe.nodeSelections")
        await disconnect()
        await c.logout()
    }

    private func afterAuth() async {
        let generation = authGeneration
        await refreshClientFeatures(force: true)
        guard generation == authGeneration, session != nil else { return }
        await refreshHome()
        guard generation == authGeneration, session != nil else { return }
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
        catch { /* The auth form keeps submission disabled and offers retry. */ }
    }

    // ---------- home ----------

    func refreshHome() async {
        guard session != nil, let c = client else { return }
        let generation = authGeneration
        homeRefreshing = true
        defer { homeRefreshing = false }
        async let u = c.currentUser()
        async let s = c.currentSubscribe()
        do {
            let (nextUser, nextSubscribe) = try await (u, s)
            guard generation == authGeneration else { return }
            user = nextUser
            subscribe = nextSubscribe
            await refreshNotices()
        } catch {
            if generation == authGeneration { snackbar = friendly(error) }
        }
    }

    func refreshNotices() async {
        guard session != nil, clientFeatures.enabled("notice"), let c = client else { notices = []; return }
        let generation = authGeneration
        do { let value = try await c.fetchNotices(); if generation == authGeneration { notices = value } }
        catch { if generation == authGeneration { snackbar = friendly(error) } }
    }

    // ---------- plans / orders ----------

    func refreshPlans() async {
        guard session != nil, clientFeatures.enabled("purchase"), let c = client else { plans = []; return }
        let generation = authGeneration
        listsRefreshing = true
        defer { listsRefreshing = false }
        do {
            let nextPlans = try await c.fetchPlans()
            let nextMethods = try await c.fetchPaymentMethods()
            guard generation == authGeneration else { return }
            plans = nextPlans
            paymentMethods = nextMethods
        } catch {
            if generation == authGeneration { snackbar = friendly(error) }
        }
    }

    func refreshOrders() async {
        guard session != nil, let c = client else { return }
        let generation = authGeneration
        listsRefreshing = true
        defer { listsRefreshing = false }
        do { let value = try await c.fetchOrders(); if generation == authGeneration { orders = value } }
        catch { if generation == authGeneration { snackbar = friendly(error) } }
    }

    func saveOrder(_ args: SaveOrderArgs) async throws -> String {
        guard session != nil, clientFeatures.enabled("purchase"), let c = client else { throw AppError.notReady }
        let generation = authGeneration
        let result = try await c.saveOrder(args: args)
        guard generation == authGeneration else { throw CancellationError() }
        return result
    }

    func checkout(_ tradeNo: String, methodId: Int64) async throws -> CheckoutResponse {
        let c = try requireClient()
        let generation = authGeneration
        let result = try await c.checkoutOrder(tradeNo: tradeNo, methodId: methodId)
        guard generation == authGeneration else { throw CancellationError() }
        return result
    }

    func checkOrderStatus(_ tradeNo: String) async throws -> Int32 {
        let c = try requireClient()
        let generation = authGeneration
        let result = try await c.checkOrder(tradeNo: tradeNo)
        guard generation == authGeneration else { throw CancellationError() }
        return result
    }

    func cancelOrder(_ tradeNo: String) async {
        guard let c = try? requireClient() else { return }
        let generation = authGeneration
        do { try await c.cancelOrder(tradeNo: tradeNo) }
        catch { if generation == authGeneration { snackbar = friendly(error) } }
    }

    // ---------- tickets ----------

    func refreshTickets() async {
        guard session != nil, clientFeatures.enabled("tickets"), let c = client else { tickets = []; return }
        let generation = authGeneration
        listsRefreshing = true
        defer { listsRefreshing = false }
        do { let value = try await c.fetchTickets(); if generation == authGeneration { tickets = value } }
        catch { if generation == authGeneration { snackbar = friendly(error) } }
    }

    func openTicket(id: Int64) async {
        guard session != nil, clientFeatures.enabled("tickets"), let c = client else { return }
        let generation = authGeneration
        do { let value = try await c.fetchTicket(id: id); if generation == authGeneration { ticketDetail = value } }
        catch { if generation == authGeneration { snackbar = friendly(error) } }
    }

    func replyTicket(id: Int64, message: String) async {
        guard session != nil, clientFeatures.enabled("tickets"), let c = client else { return }
        let generation = authGeneration
        do {
            try await c.replyTicket(id: id, message: message)
            guard generation == authGeneration else { return }
            await openTicket(id: id)
        } catch {
            if generation == authGeneration { snackbar = friendly(error) }
        }
    }

    func closeTicket(id: Int64) async {
        guard session != nil, clientFeatures.enabled("tickets"), let c = client else { return }
        let generation = authGeneration
        do {
            try await c.closeTicket(id: id)
            guard generation == authGeneration else { return }
            await openTicket(id: id)
        } catch {
            if generation == authGeneration { snackbar = friendly(error) }
        }
    }

    func saveTicket(_ args: SaveTicketArgs) async {
        guard session != nil, clientFeatures.enabled("tickets"), let c = client else { return }
        let generation = authGeneration
        do {
            _ = try await c.saveTicket(args: args)
            guard generation == authGeneration else { return }
            await refreshTickets()
        } catch {
            if generation == authGeneration { snackbar = friendly(error) }
        }
    }

    // ---------- connection ----------

    /// Toggle the OS VPN state. iOS handles spawning the NE provider — we
    /// just ask `NETunnelProviderManager` to start, with a freshly rendered
    /// sing-box config in the App Group.
    func connect() async {
        if case .connecting = connectionState { return }
        if case .connected = connectionState { return }
        guard session != nil, let client else { return }
        connectionGeneration += 1
        let attempt = connectionGeneration
        do {
            connectionState = .connecting(stage: .fetching, mode: requestedMode)
            // The NE provider doesn't have FFI access — it can't call
            // `current_subscribe()` itself. The main app fetches the YAML
            // and writes the JSON-rendered config to UserDefaults the
            // extension can read via `suiteName`.
            let account = try await client.currentUser()
            guard attempt == connectionGeneration else { return }
            guard !account.banned else { throw AppError.unavailable("账户已停用，请联系客户支持。") }
            let info = try await client.currentSubscribe()
            guard attempt == connectionGeneration else { return }
            if let expires = info.expiredAt, expires > 0, expires <= Int64(Date().timeIntervalSince1970) {
                throw AppError.unavailable("订阅已到期，请续费后连接。")
            }
            let (used, overflow) = info.upload.addingReportingOverflow(info.download)
            guard !overflow, info.transferEnable > used else {
                throw AppError.unavailable(info.planId == nil ? "请先购买订阅，再开始连接。" : "可用流量不足，请续费或重置流量。")
            }
            user = account
            subscribe = info
            let sourceYAML = try await connectionController.fetchSubscribeYAML(subscribeURL: info.subscribeUrl)
            guard attempt == connectionGeneration else { return }
            let yaml = try await client.applyCustomRulesYaml(subscribeYaml: sourceYAML)
            guard attempt == connectionGeneration else { return }
            subscribeYaml = yaml
            updateProxySnapshot(from: yaml)

            guard attempt == connectionGeneration else { return }

            connectionState = .connecting(stage: .spawning, mode: requestedMode)
            try await connectionController.start(
                subscribeYaml: yaml,
                mode: requestedMode,
                selections: selectedOverrides
            )
            guard attempt == connectionGeneration else { return }
            connectionState = .connected(since: ISO8601DateFormatter().string(from: Date()), mode: requestedMode, mixedPort: 7890)
            startTrafficPolling()
            snackbar = String(localized: "connect.status.auto_connected")
        } catch {
            guard attempt == connectionGeneration else { return }
            snackbar = friendly(error)
            connectionState = .failed(message: friendly(error), mode: requestedMode)
        }
    }

    func disconnect() async {
        connectionGeneration += 1
        trafficTask?.cancel()
        trafficTask = nil
        traffic = nil
        await connectionController.stop()
        connectionState = .disconnected
    }

    private func startTrafficPolling() {
        guard trafficTask == nil else { return }
        trafficTask = Task { [weak self] in
            var previous: (up: UInt64, down: UInt64, time: Date)?
            while !Task.isCancelled {
                guard let self else { return }
                do {
                    let data = try await self.connectionController.queryKernel("traffic")
                    guard !Task.isCancelled else { return }
                    let up = (data["uploadTotal"] as? NSNumber)?.uint64Value ?? 0
                    let down = (data["downloadTotal"] as? NSNumber)?.uint64Value ?? 0
                    let now = Date()
                    var upRate: UInt64 = 0
                    var downRate: UInt64 = 0
                    if let previous {
                        let seconds = max(0.1, now.timeIntervalSince(previous.time))
                        upRate = UInt64(Double(up >= previous.up ? up - previous.up : 0) / seconds)
                        downRate = UInt64(Double(down >= previous.down ? down - previous.down : 0) / seconds)
                    }
                    self.traffic = TrafficStats(up: upRate, down: downRate, upTotal: up, downTotal: down)
                    previous = (up, down, now)
                } catch {
                    if !Task.isCancelled { self.traffic = nil }
                }
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }

    func setMode(_ mode: TunnelMode) {
        requestedMode = mode
    }

    func refreshProxies() async {
        if case .connected = connectionState {
            do {
                let response = try await connectionController.queryKernel("proxies")
                if let all = response["proxies"] as? [String: [String: Any]] {
                    let order = proxies.map(\.name)
                    proxies = all.compactMap { name, entry in
                        guard let members = entry["all"] as? [String], let kind = entry["type"] as? String else { return nil }
                        return ProxyGroupSnapshot(name: name, kind: kind, now: entry["now"] as? String, all: members)
                    }.sorted { (order.firstIndex(of: $0.name) ?? Int.max) < (order.firstIndex(of: $1.name) ?? Int.max) }
                    if let primary = proxies.first { updateSelectedNode(primary: primary.name, current: primary.now) }
                    return
                }
            } catch { snackbar = friendly(error) }
        }
        if let yaml = subscribeYaml {
            updateProxySnapshot(from: yaml)
            return
        }
        guard session != nil, let client else { return }
        do {
            let info = try await client.currentSubscribe()
            subscribe = info
            let yaml = try await connectionController.fetchSubscribeYAML(subscribeURL: info.subscribeUrl)
            subscribeYaml = yaml
            updateProxySnapshot(from: yaml)
        } catch {
            snackbar = friendly(error)
        }
    }

    func selectProxy(group: String, node: String) async {
        guard let target = proxies.first(where: { $0.name == group }), target.kind == "Selector", target.all.contains(node) else { return }
        if case .connected = connectionState {
            do {
                _ = try await connectionController.queryKernel("select", group: group, node: node)
            } catch { snackbar = friendly(error); return }
        }
        selectedOverrides[group] = node
        UserDefaults.standard.set(selectedOverrides, forKey: "sufe.nodeSelections")
        updateSelectedNode(primary: group, current: node)
        await refreshProxies()
    }

    func latencyTest(_ node: String) async -> UInt32 {
        do {
            let data = try await connectionController.queryKernel("latency", node: node)
            return (data["delay"] as? NSNumber)?.uint32Value ?? UInt32.max
        }
        catch { return UInt32.max }
    }

    private func updateProxySnapshot(from yaml: String) {
        // Parse the actual translated config, not ad-hoc YAML lines. This
        // excludes unsupported protocols and handles block/inline YAML equally.
        do {
            let json = try renderSingboxConfig(subscribeYaml: yaml, externalController: "127.0.0.1:9090", secret: "preview", mixedPort: 7890, mode: .tun)
            let root = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any]
            let outbounds = root?["outbounds"] as? [[String: Any]] ?? []
            proxies = outbounds.compactMap { outbound in
                guard let name = outbound["tag"] as? String, let kind = outbound["type"] as? String,
                      ["selector", "urltest"].contains(kind), let members = outbound["outbounds"] as? [String], !members.isEmpty else { return nil }
                let selected = selectedOverrides[name].flatMap { members.contains($0) ? $0 : nil }
                return ProxyGroupSnapshot(name: name, kind: kind == "selector" ? "Selector" : "URLTest", now: selected ?? (outbound["default"] as? String) ?? members.first, all: members)
            }
        } catch {
            proxies = []
            snackbar = friendly(error)
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

    func requireClient() throws -> Client {
        guard session != nil, let client else { throw AppError.notReady }
        return client
    }

    func deploymentClient() -> Client? { client }

    func friendly(_ error: Error) -> String {
        if let f = error as? FfiError {
            switch f {
            case .Network: return "连接服务器失败，请检查网络后重试。"
            case .Unauthorized:
                if session != nil {
                    let generation = authGeneration
                    Task { if generation == self.authGeneration { await self.logout() } }
                }
                return "登录已过期，请重新登录。"
            case .KernelNotRunning: return "请先连接，再进行此操作。"
            case .KernelStartTimeout: return "连接超时，请更换节点后重试。"
            case .InvalidSignature, .ChecksumMismatch: return "服务配置验证失败，请联系客户支持。"
            case let .ApiFailure(message), let .Config(message), let .Kernel(message): return message
            default: return "暂时无法完成操作，请稍后重试。"
            }
        }
        if let s = error as? StorageError {
            return String(describing: s)
        }
        return error.localizedDescription
    }
}

enum AppError: LocalizedError {
    case notReady
    case unavailable(String)
    var errorDescription: String? {
        switch self {
        case .notReady: return "当前操作暂不可用，请检查登录状态与服务配置。"
        case let .unavailable(message): return message
        }
    }
}
