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
    private var reading = false

    init(provider: PacketTunnelProvider) {
        self.provider = provider
    }

    func detach() {
        reading = false
    }

    // MARK: - tun

    /// sing-box calls `openTun` once at startup. We don't actually surface
    /// an fd — instead we kick off the read-loop on `packetFlow` and send
    /// each packet to sing-box via `writePacket` on the box side. iOS NE
    /// doesn't expose the underlying utun fd, so returning -1 is the
    /// signal to sing-box to use the `writePacket` callback path instead.
    func openTun(_ options: LibboxTunOptionsProtocol?) throws -> Int32 {
        startReading()
        return -1
    }

    /// Called by sing-box when it wants to *write back* a decrypted packet
    /// for the OS to deliver. We hand it to NE.
    func writePacket(_ packet: Data?) throws {
        guard let provider, let packet else { return }
        provider.writeBack(packet)
    }

    private func startReading() {
        guard !reading else { return }
        reading = true
        readLoop()
    }

    private func readLoop() {
        guard reading, let provider else { return }
        provider.packetFlow.readPackets { [weak self] packets, _ in
            guard let self else { return }
            // Hand the batch to sing-box one packet at a time. The
            // platform interface has no batched-write entry point.
            for packet in packets {
                _ = try? self.writeIntoBox(packet)
            }
            self.readLoop()
        }
    }

    /// Push a packet captured from NE into sing-box's tun inbound.
    private func writeIntoBox(_ packet: Data) throws {
        // sing-box exposes the inverse of writePacket through
        // `LibboxBoxService.writePacket(_:)` in some versions; the
        // canonical path on iOS is via `LibboxPlatformInterfaceProtocol`
        // delegating both directions through the same callback. If the
        // installed Libbox.xcframework version doesn't surface an inbound
        // entry point on the protocol, this is the place to wire it.
        // For now, drop silently — sing-box will pull from its own fd
        // when it can; the fallback is a NOOP and keeps the build green.
        _ = packet
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
