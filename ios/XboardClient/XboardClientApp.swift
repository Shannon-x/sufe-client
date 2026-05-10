import SwiftUI

@main
struct XboardClientApp: App {
    @State private var model = AppModel.shared

    var body: some Scene {
        WindowGroup {
            RootView(model: model)
                .environment(\.locale, Locale(identifier: "zh-Hans"))
                .task { await model.bootstrap() }
        }
    }
}
