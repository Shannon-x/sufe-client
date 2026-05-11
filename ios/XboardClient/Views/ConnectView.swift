import SwiftUI

struct ConnectView: View {
    @Bindable var model: AppModel
    @State private var nodesOpen = false

    var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                Spacer(minLength: 32)
                ToggleCircle(state: model.connectionState) {
                    Task { await toggle() }
                }
                StatusCaption(state: model.connectionState)

                ModeSwitch(mode: model.requestedMode) { newMode in
                    model.setMode(newMode)
                }

                CurrentNodeRow(name: model.selectedNode, route: model.selectedRoute) {
                    nodesOpen = true
                }

                if case .connected = model.connectionState, let t = model.traffic {
                    TrafficCard(traffic: t)
                }
            }
            .screenPadding()
        }
        .navigationTitle(String(localized: "connect.title"))
        .navigationBarTitleDisplayMode(.inline)
        .sheet(isPresented: $nodesOpen) {
            NodesSheet(model: model)
        }
        .task { await model.refreshProxies() }
    }

    private func toggle() async {
        switch model.connectionState {
        case .disconnected, .failed:
            await model.connect()
        case .connecting, .connected:
            await model.disconnect()
        }
    }
}

// MARK: - Building blocks

private struct ToggleCircle: View {
    let state: ConnectionState
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            ZStack {
                Circle()
                    .fill(fill)
                    .frame(width: 200, height: 200)
                    .shadow(color: .black.opacity(0.08), radius: 8, y: 4)

                if case .connecting = state {
                    ProgressView()
                        .progressViewStyle(.circular)
                        .tint(.white)
                        .scaleEffect(2)
                } else {
                    Image(systemName: "power")
                        .font(.system(size: 60, weight: .light))
                        .foregroundStyle(iconColor)
                }
            }
        }
        .buttonStyle(.plain)
    }

    private var fill: Color {
        switch state {
        case .connected: return Color.accentColor
        case .connecting: return Color.orange
        case .disconnected, .failed: return Color(.tertiarySystemFill)
        }
    }

    private var iconColor: Color {
        switch state {
        case .connected: return .white
        case .disconnected, .failed: return .secondary
        case .connecting: return .white
        }
    }
}

private struct StatusCaption: View {
    let state: ConnectionState
    var body: some View {
        Text(text)
            .font(.callout)
            .foregroundStyle(.secondary)
    }
    private var text: String {
        switch state {
        case .disconnected:
            return String(localized: "connect.status.disconnected")
        case let .connecting(stage, _):
            switch stage {
            case .fetching: return String(localized: "connect.status.fetching")
            case .writing: return String(localized: "connect.status.writing")
            case .elevating, .fallbackProxy: return String(localized: "connect.status.elevating")
            case .spawning: return String(localized: "connect.status.spawning")
            case .applyingRoute: return String(localized: "connect.status.applying_route")
            }
        case let .connected(_, mode, _):
            return mode == .tun
                ? String(localized: "connect.status.connected_tun")
                : String(localized: "connect.status.connected_proxy")
        case let .failed(message, _):
            return message
        }
    }
}

private struct ModeSwitch: View {
    let mode: TunnelMode
    let onChange: (TunnelMode) -> Void

    var body: some View {
        Picker("", selection: Binding(
            get: { mode },
            set: { onChange($0) }
        )) {
            Text(String(localized: "connect.mode.tun")).tag(TunnelMode.tun)
            Text(String(localized: "connect.mode.system_proxy")).tag(TunnelMode.systemProxy)
        }
        .pickerStyle(.segmented)
    }
}

private struct CurrentNodeRow: View {
    let name: String?
    let route: String?
    let onPick: () -> Void

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(String(localized: "connect.node.current"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(name ?? String(localized: "connect.node.none"))
                    .font(.body)
                if let route, !route.isEmpty {
                    Text(route)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
            Button(String(localized: "connect.node.pick"), action: onPick)
        }
        .padding(12)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 12))
    }
}

private struct TrafficCard: View {
    let traffic: TrafficStats
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(String(localized: "connect.traffic.title"))
                .font(.subheadline.weight(.semibold))
            HStack(spacing: 16) {
                Label("↑ \(formatBytes(traffic.up))/s", systemImage: "arrow.up")
                Label("↓ \(formatBytes(traffic.down))/s", systemImage: "arrow.down")
            }
            .font(.callout)
            HStack(spacing: 16) {
                Text("Σ↑ \(formatBytes(traffic.upTotal))")
                Text("Σ↓ \(formatBytes(traffic.downTotal))")
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 12))
    }
}
