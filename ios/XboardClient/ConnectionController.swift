import Foundation
import NetworkExtension

/// Wraps `NETunnelProviderManager` so the SwiftUI side never has to spell
/// out `protocolConfiguration.providerBundleIdentifier` etc. The UI just
/// asks `start(...)` / `stop()`.
///
/// iOS forces every call into `NETunnelProviderManager.loadAllFromPreferences`
/// before it'll let you mutate; we cache the manager between calls but
/// always re-load when the user toggles, in case Settings → VPN was used
/// to remove the profile out from under us.
final class ConnectionController {
    static let providerBundleId = "com.xboard.client.PacketTunnel"
    static let appGroupId = "group.com.xboard.client.ios"
    static let configKey = "singbox.config.json"

    private var cached: NETunnelProviderManager?

    private func defaults() -> UserDefaults? {
        UserDefaults(suiteName: Self.appGroupId)
    }

    /// Render the sing-box JSON for the current subscription, drop it in
    /// the App Group, and ask iOS to start the tunnel. The provider then
    /// reads the JSON and hands it to LibboxBoxService.
    func start(subscribeYaml yaml: String, mode: TunnelMode) async throws {
        let json = try renderSingboxConfig(
            subscribeYaml: yaml,
            externalController: "127.0.0.1:9090",
            secret: randomSecret(),
            mixedPort: 7890,
            mode: mode
        )

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
        try manager.connection.startVPNTunnel(options: [
            // Hint surfaced in PacketTunnelProvider via options[options:].
            "modeRaw": NSString(string: mode == .tun ? "tun" : "system_proxy"),
        ])
        cached = manager
    }

    func start(subscribeToken: String, backend: String, mode: TunnelMode) async throws {
        let yaml = try await fetchSubscribeYAML(subscribeToken: subscribeToken, backend: backend)
        try await start(subscribeYaml: yaml, mode: mode)
    }

    func stop() async {
        guard let m = cached ?? (try? await loadOrCreateManager()) else { return }
        try? await m.loadFromPreferences()
        m.connection.stopVPNTunnel()
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
        manager.localizedDescription = "Xboard"
        let proto = NETunnelProviderProtocol()
        proto.providerBundleIdentifier = Self.providerBundleId
        // serverAddress is shown in Settings → VPN; iOS rejects empty strings.
        proto.serverAddress = "Xboard"
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
        let (data, response) = try await URLSession.shared.data(from: url)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw NSError(domain: "Xboard", code: -1, userInfo: [NSLocalizedDescriptionKey: "subscribe HTTP \(response)"])
        }
        guard let body = String(data: data, encoding: .utf8) else {
            throw NSError(domain: "Xboard", code: -1, userInfo: [NSLocalizedDescriptionKey: "subscribe body not UTF-8"])
        }
        return body
    }

    private func randomSecret() -> String {
        var bytes = [UInt8](repeating: 0, count: 16)
        _ = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        return bytes.map { String(format: "%02x", $0) }.joined()
    }
}
