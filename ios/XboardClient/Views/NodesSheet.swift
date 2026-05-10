import SwiftUI

struct NodesSheet: View {
    @Bindable var model: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var latencies: [String: UInt32] = [:]
    @State private var probing: Set<String> = []

    var body: some View {
        NavigationStack {
            List {
                ForEach(model.proxies, id: \.name) { group in
                    Section {
                        ForEach(group.all, id: \.self) { node in
                            NodeRow(
                                node: node,
                                isSelected: group.now == node,
                                latency: latencies[node]
                            ) {
                                Task {
                                    await model.selectProxy(group: group.name, node: node)
                                    await model.refreshProxies()
                                }
                            }
                        }
                    } header: {
                        HStack {
                            Text(group.name)
                            Spacer()
                            Button(probing.contains(group.name)
                                   ? String(localized: "nodes.testing")
                                   : String(localized: "nodes.test_latency")) {
                                Task { await testGroup(group) }
                            }
                            .disabled(probing.contains(group.name))
                            .font(.caption)
                            .textCase(.none)
                        }
                    }
                }
            }
            .navigationTitle(String(localized: "nodes.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(String(localized: "common.close")) { dismiss() }
                }
            }
            .task { await model.refreshProxies() }
        }
    }

    private func testGroup(_ group: ProxyGroup) async {
        probing.insert(group.name)
        defer { probing.remove(group.name) }
        await withTaskGroup(of: (String, UInt32).self) { tg in
            for node in group.all {
                tg.addTask { (node, await model.latencyTest(node)) }
            }
            for await (node, ms) in tg {
                latencies[node] = ms
            }
        }
    }
}

private struct NodeRow: View {
    let node: String
    let isSelected: Bool
    let latency: UInt32?
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(node).font(.body)
                    if let ms = latency {
                        LatencyBadge(ms: ms)
                    }
                }
                Spacer()
                if isSelected {
                    Image(systemName: "checkmark")
                        .foregroundStyle(.tint)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

private struct LatencyBadge: View {
    let ms: UInt32
    var body: some View {
        if ms == UInt32.max {
            Text(String(localized: "nodes.timeout"))
                .font(.caption2)
                .padding(.horizontal, 6).padding(.vertical, 2)
                .background(Color.red.opacity(0.15), in: Capsule())
                .foregroundStyle(.red)
        } else {
            Text("\(ms) ms")
                .font(.caption2)
                .padding(.horizontal, 6).padding(.vertical, 2)
                .background(tint.opacity(0.15), in: Capsule())
                .foregroundStyle(tint)
        }
    }

    private var tint: Color {
        switch ms {
        case ..<200: return .green
        case ..<500: return .orange
        default: return .red
        }
    }
}
