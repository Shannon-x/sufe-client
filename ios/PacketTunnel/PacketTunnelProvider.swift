import Foundation
import Libbox
import NetworkExtension
import os.log

/// `PacketTunnelProvider` is the entry point Apple loads when the user toggles
/// the Xboard VPN profile. It hosts an embedded sing-box (via `Libbox`) and
/// pipes packets between sing-box's `tun` inbound and `NEPacketTunnelFlow`.
///
/// The sing-box JSON configuration is rendered by the main app (which has
/// access to the FFI `render_singbox_config` and the user's subscription token)
/// and dropped in the App Group's `UserDefaults` under
/// `ConnectionController.configKey`. This NE has no FFI dependency at all,
/// which keeps it well under the 50 MB iOS NE memory cap.
final class PacketTunnelProvider: NEPacketTunnelProvider {
    // The shared App Group store also used by the main app.
    private static let appGroupId = "group.com.xboard.client.ios"
    private static let configKey = "singbox.config.json"

    private var boxService: LibboxBoxService?
    private var platformInterface: XboardPlatformInterface?
    private let log = OSLog(subsystem: "com.xboard.client.PacketTunnel", category: "tunnel")

    override func startTunnel(options: [String: NSObject]?) async throws {
        let configContent = try loadConfig()

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
        try service.start()
        self.boxService = service
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

    init(provider: PacketTunnelProvider) {
        self.provider = provider
    }

    func detach() {
        // Nothing to tear down here: sing-box owns the utun fd it obtained
        // from `openTun` and closes it during `boxService.close()`.
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
    func openTun(_ options: LibboxTunOptionsProtocol?) throws -> Int32 {
        guard let fd = Self.findUtunFileDescriptor() else {
            throw NSError(
                domain: "Xboard", code: -3,
                userInfo: [NSLocalizedDescriptionKey:
                    "could not locate the NEPacketTunnelFlow utun file descriptor"]
            )
        }
        return fd
    }

    /// sing-box→OS path. Unused once `openTun` returns a real fd (sing-box
    /// writes packets to the fd directly), but kept as a protocol-conforming
    /// fallback for Libbox builds that still call it.
    func writePacket(_ packet: Data?) throws {
        guard let provider, let packet else { return }
        provider.writeBack(packet)
    }

    /// Find the file descriptor of the `utun` interface NE created for this
    /// extension. NE doesn't expose it directly, so we scan our open fds for
    /// the one whose peer is the `com.apple.net.utun_control` kernel control
    /// socket — the same approach wireguard-apple uses. Must run inside the
    /// NE process (the fd table is per-process).
    private static func findUtunFileDescriptor() -> Int32? {
        var ctlInfo = ctl_info()
        withUnsafeMutablePointer(to: &ctlInfo.ctl_name) {
            $0.withMemoryRebound(to: CChar.self, capacity: MemoryLayout.size(ofValue: $0.pointee)) {
                _ = strcpy($0, "com.apple.net.utun_control")
            }
        }
        for fd: Int32 in 0...1024 {
            var addr = sockaddr_ctl()
            var len = socklen_t(MemoryLayout.size(ofValue: addr))
            let ret = withUnsafeMutablePointer(to: &addr) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    getpeername(fd, $0, &len)
                }
            }
            if ret != 0 || addr.sc_family != AF_SYSTEM {
                continue
            }
            if ctlInfo.ctl_id == 0 {
                _ = ioctl(fd, CTLIOCGINFO, &ctlInfo)
            }
            if addr.sc_id == ctlInfo.ctl_id {
                return fd
            }
        }
        return nil
    }

    // MARK: - interface monitor / auto-detect (delegate to platform)

    func usePlatformAutoDetectInterfaceControl() -> Bool { true }

    func autoDetectInterfaceControl(_ fd: Int32) throws {
        // Honor sing-box's request to bind a socket to the system's
        // currently-active outbound interface. NEProvider gives us
        // limited access here; we rely on `setTunnelNetworkSettings`
        // having already rewritten the routing table.
    }

    func usePlatformDefaultInterfaceMonitor() -> Bool { false }

    func startDefaultInterfaceMonitor(_ listener: LibboxInterfaceUpdateListenerProtocol?) throws {}
    func closeDefaultInterfaceMonitor(_ listener: LibboxInterfaceUpdateListenerProtocol?) throws {}

    func getInterfaces() throws -> LibboxNetworkInterfaceIteratorProtocol {
        return EmptyInterfaceIterator()
    }

    // MARK: - misc protocol stubs

    func underNetworkExtension() -> Bool { true }
    func includeAllNetworks() -> Bool { false }
    func readWIFIState() -> LibboxWIFIStateProtocol? { nil }
    func usePackageMode() -> Bool { false }
    func packageNameByUid(_ uid: Int32) throws -> String { "" }
    func uidByPackageName(_ packageName: String?) throws -> Int32 { 0 }
    func clashModeCallback(_ callback: LibboxClashModeCallbackProtocol?) throws {}
    func systemCertificates() -> LibboxStringIteratorProtocol? { nil }
    func usePlatformInterfaceGetter() -> Bool { false }
    func sendNotification(_ notification: LibboxNotificationProtocol?) throws {}
}

/// Empty iterator we can return when sing-box asks for the list of
/// network interfaces — we don't enumerate them on iOS because NE
/// doesn't expose them with useful precision.
private final class EmptyInterfaceIterator: NSObject, LibboxNetworkInterfaceIteratorProtocol {
    func hasNext() -> Bool { false }
    func next() -> LibboxNetworkInterfaceProtocol? { nil }
}
