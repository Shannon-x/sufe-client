import SwiftUI

/// Top-level switch: signed-in users see the main tab bar, everyone else
/// sees the Login surface.
struct RootView: View {
    @Bindable var model: AppModel
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        Group {
            if model.session != nil {
                MainTabs(model: model)
            } else {
                LoginView(model: model)
            }
        }
        .alert(
            String(localized: "common.notice"),
            isPresented: .init(
                get: { model.snackbar != nil },
                set: { if !$0 { model.snackbar = nil } }
            ),
            actions: {
                Button(String(localized: "common.ok"), role: .cancel) {}
            },
            message: {
                Text(model.snackbar ?? "")
            }
        )
        .tint(ProtonStyle.accent)
        .preferredColorScheme(.light)
        .task(id: "\(model.session?.email ?? "")-\(scenePhase)") {
            guard scenePhase == .active else { model.chatViewing = false; return }
            while !Task.isCancelled {
                await model.refreshClientFeatures()
                if model.session != nil { await model.refreshChat() }
                do { try await Task.sleep(for: .seconds(8)) } catch { return }
            }
        }
    }
}

private struct MainTabs: View {
    @Bindable var model: AppModel
    @State private var selected = "home"

    var body: some View {
        TabView(selection: $selected) {
            NavigationStack {
                HomeView(model: model)
            }
            .tabItem {
                Label(String(localized: "tabs.home"), systemImage: "house")
            }
            .tag("home")

            NavigationStack {
                ConnectView(model: model)
            }
            .tabItem {
                Label("节点", systemImage: "globe")
            }
            .tag("nodes")

            if model.clientFeatures.enabled("purchase") {
                NavigationStack { PlansView(model: model) }
                    .tabItem { Label(String(localized: "tabs.plans"), systemImage: "creditcard") }
                    .tag("plans")
            }

            if model.clientFeatures.enabled("chatwoot") || model.clientFeatures.enabled("tickets") {
                NavigationStack { SupportView(model: model) }
                    .tabItem { Label("客服", systemImage: "bubble.left.and.bubble.right") }
                    .badge(model.chatUnread)
                    .tag("support")
            }
            NavigationStack { AccountView(model: model) }
                .tabItem { Label("我的", systemImage: "person.crop.circle") }
                .tag("account")
        }
        .onChange(of: model.clientFeatures.features) { _, features in
            if (selected == "plans" && features["purchase"] != true) ||
                (selected == "support" && features["tickets"] != true && features["chatwoot"] != true) {
                selected = "home"
            }
        }
    }
}
