import Foundation
import NetworkExtension
import Security

/// Wraps `NETunnelProviderManager` so the SwiftUI side never has to spell
/// out `protocolConfiguration.providerBundleIdentifier` etc. The UI just
/// asks `start(...)` / `stop()`.
///
/// iOS forces every call into `NETunnelProviderManager.loadAllFromPreferences`
/// before it'll let you mutate; we cache the manager between calls but
/// always re-load when the user toggles, in case Settings → VPN was used
/// to remove the profile out from under us.
@MainActor
final class ConnectionController {
    static let providerBundleId = "com.xboard.client.PacketTunnel"
    static let appGroupId = "group.com.xboard.client.ios"
    static let configKey = "singbox.config.json"

    private var cached: NETunnelProviderManager?
    private var statusObserver: NSObjectProtocol?
    private var generation = 0
    var onStatusChange: ((NEVPNStatus) -> Void)?

    init() {
        statusObserver = NotificationCenter.default.addObserver(forName: .NEVPNStatusDidChange, object: nil, queue: .main) { [weak self] notification in
            guard let connection = notification.object as? NEVPNConnection else { return }
            Task { @MainActor [weak self] in
                guard let self, connection === self.cached?.connection else { return }
                self.onStatusChange?(connection.status)
            }
        }
    }

    deinit {
        if let statusObserver { NotificationCenter.default.removeObserver(statusObserver) }
    }

    func restoreStatus() async {
        guard let managers = try? await NETunnelProviderManager.loadAllFromPreferences() else { return }
        cached = managers.first { ($0.protocolConfiguration as? NETunnelProviderProtocol)?.providerBundleIdentifier == Self.providerBundleId }
        onStatusChange?(cached?.connection.status ?? .disconnected)
    }

    private func defaults() -> UserDefaults? {
        UserDefaults(suiteName: Self.appGroupId)
    }

    /// Render the sing-box JSON for the current subscription, drop it in
    /// the App Group, and ask iOS to start the tunnel. The provider then
    /// reads the JSON and hands it to LibboxBoxService.
    func start(subscribeYaml yaml: String, mode: TunnelMode, selections: [String: String] = [:]) async throws {
        guard mode == .tun else {
            throw NSError(domain: "Sufe", code: -4, userInfo: [NSLocalizedDescriptionKey: "iOS 仅支持 VPN 隧道模式"])
        }
        generation += 1
        let attempt = generation
        var json = try renderSingboxConfig(
            subscribeYaml: yaml,
            externalController: "127.0.0.1:9090",
            secret: randomSecret(),
            mixedPort: 7890,
            mode: mode
        )
        let parsed = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any]
        let outbounds = parsed?["outbounds"] as? [[String: Any]] ?? []
        guard outbounds.contains(where: { item in
            guard let kind = item["type"] as? String else { return false }
            return !["direct", "block", "selector", "urltest", "dns"].contains(kind)
        }) else {
            throw NSError(domain: "Sufe", code: -5, userInfo: [NSLocalizedDescriptionKey: "订阅没有 iOS 支持的可用节点，请检查套餐或联系客服"])
        }
        if var document = parsed {
            document["outbounds"] = outbounds.map { original -> [String: Any] in
                var outbound = original
                if outbound["type"] as? String == "selector", let tag = outbound["tag"] as? String,
                   let selected = selections[tag], let members = outbound["outbounds"] as? [String], members.contains(selected) {
                    outbound["default"] = selected
                }
                return outbound
            }
            json = String(decoding: try JSONSerialization.data(withJSONObject: document), as: UTF8.self)
        }

        guard let store = defaults() else {
            throw NSError(
                domain: "Xboard",
                code: -1,
                userInfo: [NSLocalizedDescriptionKey: "App Group store unavailable"]
            )
        }
        store.set(json, forKey: Self.configKey)

        let manager = try await loadOrCreateManager()
        try await manager.loadFromPreferences()
        guard attempt == generation else { throw CancellationError() }
        cached = manager
        if !manager.isEnabled {
            manager.isEnabled = true
            try await manager.saveToPreferences()
            try await manager.loadFromPreferences()
        }
        try Task.checkCancellation()
        guard attempt == generation else { throw CancellationError() }
        try manager.connection.startVPNTunnel(options: [
            // Hint surfaced in PacketTunnelProvider via options[options:].
            "modeRaw": NSString(string: mode == .tun ? "tun" : "system_proxy"),
        ])
        // startVPNTunnel only submits a request. Confirm the extension has
        // actually started before reporting success to the app.
        for tick in 0..<120 {
            try Task.checkCancellation()
            guard attempt == generation else { throw CancellationError() }
            switch manager.connection.status {
            case .connected: return
            case .invalid:
                throw NSError(domain: "Sufe", code: -6, userInfo: [NSLocalizedDescriptionKey: "VPN 配置无效，请重新连接以安装配置"])
            case .disconnected where tick > 4:
                throw NSError(domain: "Sufe", code: -6, userInfo: [NSLocalizedDescriptionKey: "VPN 扩展未能启动，请检查网络后重试"])
            default: break
            }
            try await Task.sleep(for: .milliseconds(250))
        }
        manager.connection.stopVPNTunnel()
        throw NSError(domain: "Sufe", code: -7, userInfo: [NSLocalizedDescriptionKey: "VPN 连接超时，请稍后重试"])
    }

    func start(subscribeToken: String, backend: String, mode: TunnelMode) async throws {
        let yaml = try await fetchSubscribeYAML(subscribeToken: subscribeToken, backend: backend)
        try await start(subscribeYaml: yaml, mode: mode)
    }

    func stop() async {
        generation += 1
        if cached == nil {
            let existing = (try? await NETunnelProviderManager.loadAllFromPreferences()) ?? []
            cached = existing.first { ($0.protocolConfiguration as? NETunnelProviderProtocol)?.providerBundleIdentifier == Self.providerBundleId }
        }
        guard let m = cached else { return }
        try? await m.loadFromPreferences()
        m.connection.stopVPNTunnel()
        for _ in 0..<20 {
            if [.disconnected, .invalid].contains(m.connection.status) { break }
            try? await Task.sleep(for: .milliseconds(200))
        }
    }

    func queryKernel(_ operation: String, group: String? = nil, node: String? = nil) async throws -> [String: Any] {
        guard let session = cached?.connection as? NETunnelProviderSession, session.status == .connected else {
            throw NSError(domain: "Sufe", code: -13, userInfo: [NSLocalizedDescriptionKey: "请先连接 VPN"])
        }
        var message = ["operation": operation]
        message["group"] = group
        message["node"] = node
        let encoded = try JSONSerialization.data(withJSONObject: message)
        let response: Data = try await withCheckedThrowingContinuation { continuation in
            let pending = PendingProviderRequest(continuation)
            Task { @MainActor in
                try? await Task.sleep(for: .seconds(12))
                pending.finish(.failure(URLError(.timedOut)))
            }
            do {
                try session.sendProviderMessage(encoded) { data in
                    Task { @MainActor in
                        guard let data else { pending.finish(.failure(URLError(.cannotConnectToHost))); return }
                        guard data.count <= 4 * 1024 * 1024 else { pending.finish(.failure(URLError(.dataLengthExceedsMaximum))); return }
                        pending.finish(.success(data))
                    }
                }
            } catch { pending.finish(.failure(error)) }
        }
        guard let envelope = try JSONSerialization.jsonObject(with: response) as? [String: Any], envelope["ok"] as? Bool == true else {
            throw NSError(domain: "Sufe", code: -14, userInfo: [NSLocalizedDescriptionKey: "内核操作失败，请检查连接后重试"])
        }
        return (envelope["data"] as? [String: Any]) ?? [:]
    }

    // ---------- internals ----------

    private func loadOrCreateManager() async throws -> NETunnelProviderManager {
        let managers = try await NETunnelProviderManager.loadAllFromPreferences()
        if let existing = managers.first(where: {
            ($0.protocolConfiguration as? NETunnelProviderProtocol)?
                .providerBundleIdentifier == Self.providerBundleId
        }) {
            return existing
        }

        let manager = NETunnelProviderManager()
        manager.localizedDescription = "Sufe"
        let proto = NETunnelProviderProtocol()
        proto.providerBundleIdentifier = Self.providerBundleId
        // serverAddress is shown in Settings → VPN; iOS rejects empty strings.
        proto.serverAddress = "Sufe"
        manager.protocolConfiguration = proto
        manager.isEnabled = true

        try await manager.saveToPreferences()
        try await manager.loadFromPreferences()
        return manager
    }

    func fetchSubscribeYAML(subscribeToken: String, backend: String) async throws -> String {
        // The Xboard panel returns the mihomo YAML at `<backend>/api/v1/client/subscribe?token=...&flag=clash.meta`.
        // We hit it directly here (not through the UniFFI Client) because the
        // raw YAML body isn't surfaced on the FFI — only the parsed
        // SubscribeInfo. The token alone is enough to fetch.
        guard var components = URLComponents(string: backend) else {
            throw NSError(domain: "Xboard", code: -1, userInfo: [NSLocalizedDescriptionKey: "bad backend URL"])
        }
        components.path = (components.path.hasSuffix("/") ? components.path : components.path + "/") + "api/v1/client/subscribe"
        components.queryItems = [
            URLQueryItem(name: "token", value: subscribeToken),
            URLQueryItem(name: "flag", value: "clash.meta"),
        ]
        guard let url = components.url else {
            throw NSError(domain: "Xboard", code: -1, userInfo: [NSLocalizedDescriptionKey: "bad subscribe URL"])
        }
        return try await fetchSubscribeYAML(subscribeURL: url.absoluteString)
    }

    /// The backend may return an encrypted path or a separate subscription
    /// host. Preserve that opaque URL instead of rebuilding /api/v1 locally.
    func fetchSubscribeYAML(subscribeURL: String) async throws -> String {
        guard var components = URLComponents(string: subscribeURL),
              components.scheme == "https", components.host != nil else {
            throw NSError(domain: "Sufe", code: -12, userInfo: [NSLocalizedDescriptionKey: "订阅地址必须使用 HTTPS"])
        }
        var query = components.queryItems ?? []
        query.removeAll { $0.name == "flag" }
        query.append(URLQueryItem(name: "flag", value: "clash.meta"))
        components.queryItems = query
        guard let url = components.url else { throw URLError(.badURL) }
        var request = URLRequest(url: url)
        request.timeoutInterval = 25
        request.setValue("clash.meta Sufe-iOS/0.1", forHTTPHeaderField: "User-Agent")
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            let message = [401, 403].contains(status) ? "订阅暂不可用，请检查套餐有效期和剩余流量" : "订阅获取失败（HTTP \(status)），请稍后重试"
            throw NSError(domain: "Sufe", code: status, userInfo: [NSLocalizedDescriptionKey: message])
        }
        guard let body = String(data: data, encoding: .utf8) else {
            throw NSError(domain: "Xboard", code: -1, userInfo: [NSLocalizedDescriptionKey: "subscribe body not UTF-8"])
        }
        return body
    }

    private func randomSecret() -> String {
        var bytes = [UInt8](repeating: 0, count: 16)
        guard SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) == errSecSuccess else {
            // UUID uses the system random source as an independent fallback.
            return UUID().uuidString + UUID().uuidString
        }
        return bytes.map { String(format: "%02x", $0) }.joined()
    }
}

/// Reply and timeout race on MainActor; the checked continuation is resumed once.
@MainActor
private final class PendingProviderRequest {
    private var continuation: CheckedContinuation<Data, Error>?
    init(_ continuation: CheckedContinuation<Data, Error>) { self.continuation = continuation }
    func finish(_ result: Result<Data, Error>) {
        guard let continuation else { return }
        self.continuation = nil
        continuation.resume(with: result)
    }
}
