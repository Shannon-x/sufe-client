import SwiftUI

struct HomeView: View {
    @Bindable var model: AppModel

    var body: some View {
        List {
            Section {
                if let info = model.subscribe {
                    SubscribeCard(info: info)
                } else if model.homeRefreshing {
                    ProgressView()
                } else {
                    Text(String(localized: "home.subscribe.empty"))
                        .foregroundStyle(.secondary)
                }
            }

            Section {
                NavigationLink {
                    OrdersView(model: model)
                } label: {
                    Label(String(localized: "home.menu.orders"), systemImage: "cart")
                }
                NavigationLink {
                    NoticesView(model: model)
                } label: {
                    Label(String(localized: "home.menu.notices"), systemImage: "bell")
                }
            }

            Section {
                if let user = model.user {
                    LabeledContent(String(localized: "home.user.email"), value: user.email)
                    LabeledContent(
                        String(localized: "home.user.balance"),
                        value: formatPriceCents(user.balance)
                    )
                    if let exp = user.expiredAt {
                        LabeledContent(
                            String(localized: "home.user.expires_at"),
                            value: formatDateTime(exp)
                        )
                    }
                }
            }

            Section {
                Button(role: .destructive) {
                    Task { await model.logout() }
                } label: {
                    Label(String(localized: "home.logout"), systemImage: "rectangle.portrait.and.arrow.right")
                }
            }
        }
        .navigationTitle(String(localized: "tabs.home"))
        .refreshable { await model.refreshHome() }
        .task { await model.refreshHome() }
    }
}

private struct SubscribeCard: View {
    let info: SubscribeInfo

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(String(localized: "home.subscribe.title"))
                .font(.headline)

            let used = info.upload + info.download
            let total = max(info.transferEnable, 1)
            ProgressView(value: Double(used), total: Double(total))
                .tint(.accentColor)

            HStack {
                Text("\(formatBytes(used)) / \(formatBytes(info.transferEnable))")
                Spacer()
                if let exp = info.expiredAt {
                    Text(formatDateTime(exp)).foregroundStyle(.secondary)
                }
            }
            .font(.caption)
        }
        .padding(.vertical, 4)
    }
}
