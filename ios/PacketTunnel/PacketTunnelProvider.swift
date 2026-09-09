import Foundation
import Libbox
import NetworkExtension
import Network
import os.log

/// `PacketTunnelProvider` is the entry point Apple loads when the user toggles
/// the Xboard VPN profile. It hosts an embedded sing-box (via `Libbox`) and
/// pipes packets between sing-box's `tun` inbound and `NEPacketTunnelFlow`.
///
/// The sing-box JSON configuration is rendered by the main app (which has
/// access to the FFI `render_singbox_config` and the user's subscription token)
/// and dropped in the App Group's `UserDefaults` under
/// `ConnectionController.configKey`. The extension's memory use must still
/// be profiled on physical devices before distribution.
final class PacketTunnelProvider: NEPacketTunnelProvider {
    // The shared App Group store also used by the main app.
    private static let appGroupId = "group.com.xboard.client.ios"
    private static let configKey = "singbox.config.json"

    private var boxService: LibboxBoxService?
    private var platformInterface: XboardPlatformInterface?
    private let log = OSLog(subsystem: "com.xboard.client.PacketTunnel", category: "tunnel")

    override func startTunnel(options: [String: NSObject]?) async throws {
        guard LibboxVersion() == "1.10.7" || LibboxVersion() == "v1.10.7" else {
            throw NSError(domain: "Sufe", code: -8, userInfo: [NSLocalizedDescriptionKey: "Libbox 版本与客户端配置不匹配，请重新安装客户端"])
        }
        let configContent = try loadConfig()
        let fileManager = FileManager.default
        guard let base = fileManager.containerURL(forSecurityApplicationGroupIdentifier: Self.appGroupId) else {
            throw NSError(domain: "Sufe", code: -9, userInfo: [NSLocalizedDescriptionKey: "VPN 共享容器不可用"])
        }
        let setup = LibboxSetupOptions()
        setup.basePath = base.path
        setup.workingPath = base.appendingPathComponent("kernel", isDirectory: true).path
        setup.tempPath = fileManager.temporaryDirectory.path
        var setupError: NSError?
        LibboxSetup(setup, &setupError)
        if let setupError { throw setupError }


        let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "172.19.0.1")
        settings.mtu = 1500

        let ipv4 = NEIPv4Settings(addresses: ["172.19.0.1"], subnetMasks: ["255.255.255.252"])
        ipv4.includedRoutes = [NEIPv4Route.default()]
        settings.ipv4Settings = ipv4

        let ipv6 = NEIPv6Settings(addresses: ["fdfe:dcba:9876::1"], networkPrefixLengths: [126])
        ipv6.includedRoutes = [NEIPv6Route.default()]
        settings.ipv6Settings = ipv6

        let dns = NEDNSSettings(servers: ["1.1.1.1", "223.5.5.5"])
        dns.matchDomains = [""]
        settings.dnsSettings = dns

        try await setTunnelNetworkSettings(settings)

        let pi = XboardPlatformInterface(provider: self)
        self.platformInterface = pi

        var error: NSError?
        guard let service = LibboxNewService(configContent, pi, &error) else {
            throw error ?? NSError(
                domain: "Xboard", code: -1,
                userInfo: [NSLocalizedDescriptionKey: "LibboxNewService returned nil"]
            )
        }
        do {
            try service.start()
            self.boxService = service
        } catch {
            try? service.close()
            pi.detach()
            platformInterface = nil
            throw error
        }
        os_log("sing-box started", log: log, type: .info)
    }

    override func stopTunnel(with reason: NEProviderStopReason) async {
        os_log("stopTunnel reason=%{public}@", log: log, type: .info,
               String(describing: reason))
        try? boxService?.close()
        boxService = nil
        platformInterface?.detach()
        platformInterface = nil
    }

    /// App-to-extension RPC keeps the controller secret inside the shared
    /// trusted container and only exposes the operations the UI needs.
    override func handleAppMessage(_ messageData: Data, completionHandler: ((Data?) -> Void)?) {
        Task {
            do {
                guard boxService != nil,
                      let message = try JSONSerialization.jsonObject(with: messageData) as? [String: String],
                      let operation = message["operation"] else { throw URLError(.cannotConnectToHost) }
                let config = try JSONSerialization.jsonObject(with: Data(loadConfig().utf8)) as? [String: Any]
                let experimental = config?["experimental"] as? [String: Any]
                let api = experimental?["clash_api"] as? [String: Any]
                guard let address = api?["external_controller"] as? String,
                      address == "127.0.0.1:9090", let secret = api?["secret"] as? String else { throw URLError(.badURL) }
                var components = URLComponents(string: "http://127.0.0.1:9090")!
                var body: Data?
                let segmentCharacters = CharacterSet.urlPathAllowed.subtracting(CharacterSet(charactersIn: "/?#%"))
                switch operation {
                case "proxies": components.path = "/proxies"
                case "traffic": components.path = "/connections"
                case "latency":
                    guard let node = message["node"], !node.isEmpty else { throw URLError(.badURL) }
                    guard let encoded = node.addingPercentEncoding(withAllowedCharacters: segmentCharacters) else { throw URLError(.badURL) }
                    components.percentEncodedPath = "/proxies/\(encoded)/delay"
                    components.queryItems = [URLQueryItem(name: "url", value: "https://www.gstatic.com/generate_204"), URLQueryItem(name: "timeout", value: "5000")]
                case "select":
                    guard let group = message["group"], let node = message["node"], !group.isEmpty, !node.isEmpty else { throw URLError(.badURL) }
                    guard let encoded = group.addingPercentEncoding(withAllowedCharacters: segmentCharacters) else { throw URLError(.badURL) }
                    components.percentEncodedPath = "/proxies/\(encoded)"
                    body = try JSONSerialization.data(withJSONObject: ["name": node])
                default: throw URLError(.unsupportedURL)
                }
                guard let url = components.url else { throw URLError(.badURL) }
                var request = URLRequest(url: url)
                request.timeoutInterval = 8
                request.httpMethod = body == nil ? "GET" : "PUT"
                request.httpBody = body
                request.setValue("Bearer \(secret)", forHTTPHeaderField: "Authorization")
                request.setValue("application/json", forHTTPHeaderField: "Content-Type")
                let (data, response) = try await URLSession.shared.data(for: request)
                guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else { throw URLError(.badServerResponse) }
                var payload: Any
                if data.isEmpty { payload = NSNull() }
                else { payload = try JSONSerialization.jsonObject(with: data) }
                if operation == "traffic", let totals = payload as? [String: Any] {
                    payload = ["uploadTotal": totals["uploadTotal"] ?? 0, "downloadTotal": totals["downloadTotal"] ?? 0]
                }
                completionHandler?(try JSONSerialization.data(withJSONObject: ["ok": true, "data": payload]))
            } catch {
                completionHandler?(try? JSONSerialization.data(withJSONObject: ["ok": false, "error": error.localizedDescription]))
            }
        }
    }

    // MARK: - helpers

    private func loadConfig() throws -> String {
        guard let store = UserDefaults(suiteName: Self.appGroupId),
              let content = store.string(forKey: Self.configKey),
              !content.isEmpty
        else {
            throw NSError(
                domain: "Xboard", code: -2,
                userInfo: [NSLocalizedDescriptionKey:
                    "No sing-box config in app group \(Self.appGroupId)"]
            )
        }
        return content
    }

    /// Called by `XboardPlatformInterface.writePacket` — sing-box wants to
    /// hand a packet back up the stack. We forward it through `packetFlow`.
    func writeBack(_ packet: Data) {
        // IPv4 = 0x4_, IPv6 = 0x6_; NE expects AF_INET / AF_INET6.
        guard let firstByte = packet.first else { return }
        let proto: NSNumber = (firstByte >> 4 == 6) ? AF_INET6 as NSNumber : AF_INET as NSNumber
        packetFlow.writePackets([packet], withProtocols: [proto])
    }
}

/// Bridges sing-box's `LibboxPlatformInterfaceProtocol` to our NE provider.
/// Only the methods sing-box actually invokes on iOS are wired; the rest
/// are sane no-ops (this matches the `ExtensionPlatformInterface` shape
/// upstream sing-box-for-apple uses).
final class XboardPlatformInterface: NSObject, LibboxPlatformInterfaceProtocol {
    private weak var provider: PacketTunnelProvider?
    private var networkMonitor: NWPathMonitor?

    init(provider: PacketTunnelProvider) {
        self.provider = provider
    }

    func detach() {
        networkMonitor?.cancel()
        networkMonitor = nil
        // Libbox duplicates the borrowed NE fd and closes its own duplicate.
        // NetworkExtension retains ownership of packetFlow's original fd.
    }

    // MARK: - tun

    /// sing-box calls `openTun` once at startup and expects a real file
    /// descriptor it can read/write packets through directly.
    ///
    /// The previous implementation returned -1 and tried to shuttle packets
    /// via a read-loop into `writeIntoBox`, but `LibboxPlatformInterfaceProtocol`
    /// has no OS→sing-box inbound entry point — so every inbound packet was
    /// dropped and the tunnel carried no traffic (it looked "connected" but
    /// nothing worked). The correct, canonical path (matching
    /// sing-box-for-apple and wireguard-apple) is to locate the NE's backing
    /// `utun` socket fd and hand it to sing-box so it does I/O itself.
    func openTun(_ options: LibboxTunOptionsProtocol?, ret0_: UnsafeMutablePointer<Int32>?) throws {
        let fd = LibboxGetTunnelFileDescriptor()
        guard fd >= 0, let ret0_ else {
            throw NSError(
                domain: "Xboard", code: -3,
                userInfo: [NSLocalizedDescriptionKey:
                    "could not locate the NEPacketTunnelFlow utun file descriptor"]
            )
        }
        ret0_.pointee = fd
    }

    /// sing-box→OS path. Unused once `openTun` returns a real fd (sing-box
    /// writes packets to the fd directly), but kept as a protocol-conforming
    /// fallback for Libbox builds that still call it.
    func writePacket(_ packet: Data?) throws {
        guard let provider, let packet else { return }
        provider.writeBack(packet)
    }

    // MARK: - interface monitor / auto-detect (delegate to platform)

    func usePlatformAutoDetectInterfaceControl() -> Bool { false }
    // Swift's Objective-C importer shortens PlatformInterface method names
    // in gomobile's 1.10 framework. Keep the explicit aliases for the ABI.
    func usePlatformAutoDetectControl() -> Bool { false }
    func autoDetectControl(_ fd: Int32) throws {}

    func autoDetectInterfaceControl(_ fd: Int32) throws {
        // Honor sing-box's request to bind a socket to the system's
        // currently-active outbound interface. NEProvider gives us
        // limited access here; we rely on `setTunnelNetworkSettings`
        // having already rewritten the routing table.
    }

    func usePlatformDefaultInterfaceMonitor() -> Bool { true }

    func startDefaultInterfaceMonitor(_ listener: LibboxInterfaceUpdateListenerProtocol?) throws {
        guard let listener else { return }
        let monitor = NWPathMonitor()
        networkMonitor = monitor
        let ready = DispatchSemaphore(value: 0)
        monitor.pathUpdateHandler = { path in
            if path.status == .satisfied, let interface = path.availableInterfaces.first {
                listener.updateDefaultInterface(interface.name, interfaceIndex: Int32(interface.index))
            } else {
                listener.updateDefaultInterface("", interfaceIndex: -1)
            }
            ready.signal()
        }
        monitor.start(queue: DispatchQueue(label: "sufe.vpn.network"))
        guard ready.wait(timeout: .now() + 5) == .success else {
            monitor.cancel()
            networkMonitor = nil
            throw NSError(domain: "Sufe", code: -10, userInfo: [NSLocalizedDescriptionKey: "无法确定底层网络"])
        }
    }
    func closeDefaultInterfaceMonitor(_ listener: LibboxInterfaceUpdateListenerProtocol?) throws {
        detach()
    }

    func getInterfaces() throws -> LibboxNetworkInterfaceIteratorProtocol {
        return EmptyInterfaceIterator()
    }

    // MARK: - misc protocol stubs

    func underNetworkExtension() -> Bool { true }
    func includeAllNetworks() -> Bool { false }
    func readWIFIState() -> LibboxWIFIState? { nil }
    func useProcFS() -> Bool { false }
    func clearDNSCache() {}
    func writeLog(_ message: String?) {
        guard let message else { return }
        os_log("%{private}@", log: .default, type: .info, message)
    }
    func findConnectionOwner(_ ipProtocol: Int32, sourceAddress: String?, sourcePort: Int32, destinationAddress: String?, destinationPort: Int32, ret0_: UnsafeMutablePointer<Int32>?) throws {
        throw NSError(domain: "Sufe", code: -11, userInfo: [NSLocalizedDescriptionKey: "iOS 不提供应用进程归属"])
    }
    func packageName(byUid uid: Int32, error: NSErrorPointer) -> String { "" }
    func uid(byPackageName packageName: String?, ret0_: UnsafeMutablePointer<Int32>?) throws { ret0_?.pointee = 0 }
    func usePlatformInterfaceGetter() -> Bool { false }
    func useGetter() -> Bool { false }
    func send(_ notification: LibboxNotification?) throws {}
}

/// Empty iterator we can return when sing-box asks for the list of
/// network interfaces — we don't enumerate them on iOS because NE
/// doesn't expose them with useful precision.
private final class EmptyInterfaceIterator: NSObject, LibboxNetworkInterfaceIteratorProtocol {
    func hasNext() -> Bool { false }
    func next() -> LibboxNetworkInterface? { nil }
}
