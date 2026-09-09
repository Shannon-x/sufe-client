import SwiftUI

@MainActor
struct LoginView: View {
    @Bindable var model: AppModel
    @State private var email = ""
    @State private var password = ""
    @State private var showRegister = false
    @State private var showForget = false
    @State private var captcha = CaptchaController()
    @State private var submitting = false
    @State private var loadingSettings = false
    @State private var formError: String?

    private var captchaRequired: Bool { model.siteConfig?.isCaptcha ?? true }
    private var blocked: Bool {
        model.siteConfig == nil || email.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || password.isEmpty ||
        model.isAuthBusy || submitting || (captchaRequired && !captcha.canSubmit)
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 18) {
                    Image(systemName: "bolt.shield.fill")
                        .font(.system(size: 44)).foregroundStyle(.tint)
                        .padding(22).background(Color.accentColor.opacity(0.09), in: RoundedRectangle(cornerRadius: 26))
                        .padding(.top, 30)
                    Text(String(localized: "auth.login.title")).font(.largeTitle.bold())
                    Text(String(localized: "auth.login.subtitle"))
                        .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    VStack(spacing: 12) {
                        TextField(String(localized: "auth.email"), text: $email)
                            .textFieldStyle(.roundedBorder).keyboardType(.emailAddress)
                            .textContentType(.username).textInputAutocapitalization(.never).autocorrectionDisabled()
                        SecureField(String(localized: "auth.password"), text: $password)
                            .textFieldStyle(.roundedBorder).textContentType(.password)
                    }.padding(.top, 12).disabled(submitting || model.isAuthBusy)

                    AuthSecuritySection(siteConfig: model.siteConfig, controller: captcha, loading: loadingSettings) {
                        Task { await loadSettings() }
                    }
                    if let error = formError ?? model.loginError {
                        Text(error).font(.footnote).foregroundStyle(.red)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    Button(action: submit) {
                        if submitting || model.isAuthBusy { ProgressView().tint(.white) }
                        else { Text(String(localized: "auth.login.submit")).frame(maxWidth: .infinity) }
                    }
                    .buttonStyle(.borderedProminent).controlSize(.large).disabled(blocked)
                    HStack {
                        Button(String(localized: "auth.forget")) { showForget = true }
                        Spacer()
                        Button(String(localized: "auth.register.cta")) { showRegister = true }
                    }.font(.footnote).padding(.top, 4)
                }.screenPadding()
            }
            .scrollDismissesKeyboard(.interactively)
            .task { await loadSettings() }
            .onDisappear { captcha.reset() }
            .navigationDestination(isPresented: $showRegister) { RegisterView(model: model) }
            .navigationDestination(isPresented: $showForget) { ForgetPasswordView(model: model) }
        }
    }

    private func loadSettings() async {
        loadingSettings = true
        await model.bootstrap()
        await model.loadSiteConfig()
        loadingSettings = false
    }

    private func submit() {
        guard !blocked else { return }
        submitting = true
        formError = nil
        Task {
            defer { submitting = false; captcha.reset() }
            do {
                let token = try await captcha.resolve(required: captchaRequired, action: "login")
                guard !Task.isCancelled else { return }
                await model.login(email: email.trimmingCharacters(in: .whitespacesAndNewlines), password: password, captchaToken: token)
            } catch { formError = error.localizedDescription }
        }
    }
}
