import SwiftUI

struct HomeView: View {
    @Bindable var model: AppModel

    private var isConnected: Bool {
        if case .connected = model.connectionState { return true }
        return false
    }

    private var isConnecting: Bool {
        if case .connecting = model.connectionState { return true }
        return false
    }

    private var selectedLocation: GeoPoint? {
        model.selectedNode.flatMap { locateNode($0) }
    }

    private var pins: [GeoMapPin] {
        collectMapPins(groups: model.proxies, activeNode: model.selectedNode)
    }

    var body: some View {
        ZStack {
            LinearGradient(
                colors: [
                    Color(red: 0.14, green: 0.10, blue: 0.20),
                    ProtonStyle.background,
                    Color(red: 0.05, green: 0.08, blue: 0.12)
                ],
                startPoint: .top,
                endPoint: .bottom
            )
            .ignoresSafeArea()

            WorldMapCanvas(pins: pins)
                .ignoresSafeArea()
                .opacity(0.92)

            ScrollView {
                VStack(spacing: 18) {
                    statusHeader

                    if let info = model.subscribe {
                        ProtonSubscribeCard(info: info, location: selectedLocation)
                    } else if model.homeRefreshing {
                        ProgressView().tint(.white)
                    }

                    Spacer(minLength: 210)

                    selectedServerCard
                    actionRow
                }
                .padding(.horizontal, 18)
                .padding(.top, 20)
                .padding(.bottom, 30)
            }
        }
        .navigationTitle(String(localized: "tabs.home"))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .navigationBarTrailing) {
                Button {
                    Task { await model.logout() }
                } label: {
                    Image(systemName: "rectangle.portrait.and.arrow.right")
                }
                .tint(ProtonStyle.textMuted)
            }
        }
        .toolbarBackground(ProtonStyle.panel, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
        .toolbarColorScheme(.dark, for: .navigationBar)
        .refreshable {
            await model.refreshHome()
            await model.refreshProxies()
        }
        .task {
            await model.refreshHome()
            await model.refreshProxies()
        }
    }

    private var statusHeader: some View {
        VStack(spacing: 8) {
            Image(systemName: isConnected ? "lock.shield.fill" : "lock.open.fill")
                .font(.system(size: 28, weight: .bold))
                .foregroundStyle(isConnected ? ProtonStyle.green : ProtonStyle.danger)
                .padding(14)
                .background((isConnected ? ProtonStyle.green : ProtonStyle.danger).opacity(0.16), in: RoundedRectangle(cornerRadius: 16))

            Text(isConnected ? "已保护" : "未保护")
                .font(.title2.weight(.heavy))
                .foregroundStyle(isConnected ? ProtonStyle.green : ProtonStyle.danger)

            Text(isConnected ? (model.selectedNode ?? String(localized: "connect.node.current")) : "连接以保护您的隐私")
                .font(.callout)
                .foregroundStyle(ProtonStyle.textMuted)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity)
    }

    private var selectedServerCard: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("当前选择")
                .font(.caption.weight(.bold))
                .foregroundStyle(ProtonStyle.textMuted)

            NavigationLink {
                ConnectView(model: model)
            } label: {
                HStack {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(model.selectedNode ?? "最快服务器")
                            .font(.headline.weight(.heavy))
                            .foregroundStyle(.white)
                            .lineLimit(1)
                        Text(model.selectedRoute ?? selectedLocation.map { "\($0.flag) \($0.country) · \($0.label)" } ?? "自动选择最优节点")
                            .font(.caption)
                            .foregroundStyle(ProtonStyle.textMuted)
                            .lineLimit(1)
                    }
                    Spacer()
                    Image(systemName: "chevron.right")
                        .foregroundStyle(ProtonStyle.textMuted)
                }
            }
            .buttonStyle(.plain)

            Button {
                Task { await toggleConnection() }
            } label: {
                HStack {
                    Image(systemName: "power")
                    Text(isConnecting ? "连接中" : (isConnected ? "断开连接" : "快速连接"))
                }
                .font(.headline.weight(.heavy))
                .frame(maxWidth: .infinity, minHeight: 56)
                .foregroundStyle(.white)
                .background(
                    LinearGradient(
                        colors: [ProtonStyle.accent, ProtonStyle.green.opacity(0.86)],
                        startPoint: .leading,
                        endPoint: .trailing
                    ),
                    in: RoundedRectangle(cornerRadius: 14)
                )
            }
            .buttonStyle(.plain)
            .disabled(isConnecting)
        }
        .padding(18)
        .background(ProtonStyle.panel.opacity(0.88), in: RoundedRectangle(cornerRadius: 22))
        .overlay(
            RoundedRectangle(cornerRadius: 22)
                .stroke(ProtonStyle.panelBorder)
        )
    }

    private var actionRow: some View {
        HStack(spacing: 10) {
            NavigationLink {
                ConnectView(model: model)
            } label: {
                ActionTile(title: "节点", systemImage: "globe")
            }
            NavigationLink {
                OrdersView(model: model)
            } label: {
                ActionTile(title: String(localized: "home.menu.orders"), systemImage: "cart")
            }
            NavigationLink {
                NoticesView(model: model)
            } label: {
                ActionTile(title: String(localized: "home.menu.notices"), systemImage: "bell")
            }
        }
        .buttonStyle(.plain)
    }

    private func toggleConnection() async {
        switch model.connectionState {
        case .disconnected, .failed:
            await model.connect()
        case .connecting, .connected:
            await model.disconnect()
        }
    }
}

private struct ProtonSubscribeCard: View {
    let info: SubscribeInfo
    let location: GeoPoint?

    var body: some View {
        let used = info.upload + info.download
        let total = max(info.transferEnable, 1)

        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Label("流量使用", systemImage: "waveform.path.ecg")
                    .font(.headline.weight(.bold))
                    .foregroundStyle(.white)
                Spacer()
                if let exp = info.expiredAt {
                    Text(formatDateTime(exp))
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(ProtonStyle.textMuted)
                }
            }

            ProgressView(value: Double(used), total: Double(total))
                .tint(ProtonStyle.accent)
                .background(Color.white.opacity(0.10), in: Capsule())

            HStack {
                VStack(alignment: .leading, spacing: 3) {
                    Text(formatBytes(info.transferEnable - min(info.transferEnable, used)))
                        .font(.title3.weight(.heavy))
                        .foregroundStyle(.white)
                    Text("剩余流量")
                        .font(.caption)
                        .foregroundStyle(ProtonStyle.textMuted)
                }
                Spacer()
                VStack(alignment: .trailing, spacing: 3) {
                    Text(formatBytes(used))
                        .font(.title3.weight(.heavy))
                        .foregroundStyle(.white)
                    Text("本次套餐")
                        .font(.caption)
                        .foregroundStyle(ProtonStyle.textMuted)
                }
            }

            if let location {
                Text("\(location.flag) \(location.country) · \(location.label)")
                    .font(.caption.weight(.bold))
                    .foregroundStyle(ProtonStyle.green)
                    .lineLimit(1)
            }
        }
        .padding(18)
        .background(ProtonStyle.panel.opacity(0.82), in: RoundedRectangle(cornerRadius: 20))
        .overlay(
            RoundedRectangle(cornerRadius: 20)
                .stroke(ProtonStyle.panelBorder)
        )
    }
}

private struct WorldMapCanvas: View {
    let pins: [GeoMapPin]

    var body: some View {
        Canvas { context, size in
            let w = size.width
            let h = size.height
            let land = Color.black.opacity(0.32)
            let stroke = Color.white.opacity(0.18)

            func polygon(_ points: [(CGFloat, CGFloat)]) -> Path {
                var path = Path()
                guard let first = points.first else { return path }
                path.move(to: CGPoint(x: first.0 * w, y: first.1 * h))
                for point in points.dropFirst() {
                    path.addLine(to: CGPoint(x: point.0 * w, y: point.1 * h))
                }
                path.closeSubpath()
                return path
            }

            for rawLine in stride(from: 0.12, through: 0.88, by: 0.16) {
                let line = CGFloat(rawLine)
                var vertical = Path()
                vertical.move(to: CGPoint(x: w * line, y: 0))
                vertical.addLine(to: CGPoint(x: w * line, y: h))
                context.stroke(vertical, with: .color(Color.white.opacity(0.035)), lineWidth: 1)
            }

            let eurasia = polygon([(0.45, 0.18), (0.58, 0.12), (0.72, 0.20), (0.88, 0.28), (0.82, 0.48), (0.67, 0.52), (0.56, 0.62), (0.42, 0.56), (0.32, 0.42)])
            let americas = polygon([(0.17, 0.18), (0.30, 0.25), (0.27, 0.45), (0.20, 0.62), (0.12, 0.45), (0.10, 0.27)])
            let australia = polygon([(0.75, 0.66), (0.86, 0.72), (0.82, 0.84), (0.72, 0.78)])
            context.fill(eurasia, with: .color(land))
            context.stroke(eurasia, with: .color(stroke), lineWidth: 1.2)
            context.fill(americas, with: .color(land))
            context.stroke(americas, with: .color(stroke), lineWidth: 1.2)
            context.fill(australia, with: .color(land))
            context.stroke(australia, with: .color(stroke), lineWidth: 1.2)

            for pin in pins {
                let point = CGPoint(x: w * CGFloat(pin.x) / 100, y: h * CGFloat(pin.y) / 100)
                let color = pin.active ? ProtonStyle.green : ProtonStyle.danger
                let haloRadius: CGFloat = pin.active ? 18 : 12
                let dotRadius: CGFloat = pin.active ? 6 : 4
                context.fill(
                    Path(ellipseIn: CGRect(x: point.x - haloRadius, y: point.y - haloRadius, width: haloRadius * 2, height: haloRadius * 2)),
                    with: .color(color.opacity(0.18))
                )
                context.fill(
                    Path(ellipseIn: CGRect(x: point.x - dotRadius, y: point.y - dotRadius, width: dotRadius * 2, height: dotRadius * 2)),
                    with: .color(color)
                )
            }
        }
    }
}

private struct ActionTile: View {
    let title: String
    let systemImage: String

    var body: some View {
        VStack(spacing: 6) {
            Image(systemName: systemImage)
            Text(title).font(.caption.weight(.bold))
        }
        .foregroundStyle(.white)
        .frame(maxWidth: .infinity, minHeight: 58)
        .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 14))
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .stroke(ProtonStyle.panelBorder)
        )
    }
}
