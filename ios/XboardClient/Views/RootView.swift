import SwiftUI

/// Top-level switch: signed-in users see the main tab bar, everyone else
/// sees the Login surface.
struct RootView: View {
    @Bindable var model: AppModel

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
        .preferredColorScheme(.dark)
    }
}

private struct MainTabs: View {
    @Bindable var model: AppModel

    var body: some View {
        TabView {
            NavigationStack {
                HomeView(model: model)
            }
            .tabItem {
                Label(String(localized: "tabs.home"), systemImage: "house")
            }

            NavigationStack {
                ConnectView(model: model)
            }
            .tabItem {
                Label(String(localized: "tabs.connect"), systemImage: "bolt.horizontal.circle")
            }

            NavigationStack {
                PlansView(model: model)
            }
            .tabItem {
                Label(String(localized: "tabs.plans"), systemImage: "creditcard")
            }

            NavigationStack {
                TicketsView(model: model)
            }
            .tabItem {
                Label(String(localized: "tabs.tickets"), systemImage: "envelope")
            }
        }
    }
}
