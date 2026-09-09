import SwiftUI

@MainActor
struct ForgetPasswordView: View {
    @Bindable var model: AppModel
    @State private var email = ""
    @State private var emailCode = ""
    @State private var password = ""
    @State private var captcha = CaptchaController()
    @State private var sendingCode = false
    @State private var submitting = false
    @State private var loadingSettings = false
    @State private var resendAfter = Date.distantPast
    @State private var formError: String?

    private var captchaRequired: Bool { model.siteConfig?.isCaptcha ?? true }
    private var busy: Bool { submitting || sendingCode || model.isAuthBusy }
    private var canValidate: Bool { model.siteConfig != nil && (!captchaRequired || captcha.canSubmit) }
    private var blocked: Bool { email.isEmpty || password.isEmpty || emailCode.isEmpty || busy || !canValidate }

    var body: some View {
        ScrollView {
            VStack(spacing: 16) {
                TextField(String(localized: "auth.email"), text: $email)
                    .textFieldStyle(.roundedBorder).keyboardType(.emailAddress).textContentType(.username)
                    .textInputAutocapitalization(.never).autocorrectionDisabled().disabled(busy)
                HStack {
                    TextField(String(localized: "auth.email_code"), text: $emailCode)
                        .textFieldStyle(.roundedBorder).keyboardType(.numberPad).textContentType(.oneTimeCode)
                    TimelineView(.periodic(from: .now, by: 1)) { context in
                        let remaining = max(0, Int(ceil(resendAfter.timeIntervalSince(context.date))))
                        Button(action: sendCode) {
                            if sendingCode { ProgressView() }
                            else { Text(remaining > 0 ? "\(remaining) 秒后重发" : String(localized: "auth.email_code.send")) }
                        }.disabled(email.isEmpty || busy || !canValidate || remaining > 0)
                    }
                }
                SecureField(String(localized: "auth.password.new"), text: $password)
                    .textFieldStyle(.roundedBorder).textContentType(.newPassword).disabled(busy)
                AuthSecuritySection(siteConfig: model.siteConfig, controller: captcha, loading: loadingSettings) {
                    Task { await loadSettings() }
                }
                if captchaRequired {
                    Text("发码和重设密码需要分别验证。验证码只用于本次请求。")
                        .font(.caption).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
                }
                if let error = formError ?? model.loginError {
                    Text(error).font(.footnote).foregroundStyle(.red).frame(maxWidth: .infinity, alignment: .leading)
                }
                Button(action: submit) {
                    if submitting || model.isAuthBusy { ProgressView().tint(.white) }
                    else { Text(String(localized: "auth.forget.submit")).frame(maxWidth: .infinity) }
                }.buttonStyle(.borderedProminent).controlSize(.large).disabled(blocked)
            }.screenPadding()
        }
        .scrollDismissesKeyboard(.interactively)
        .navigationTitle(String(localized: "auth.forget.title"))
        .navigationBarTitleDisplayMode(.inline)
        .task { await loadSettings() }
        .onDisappear { captcha.reset() }
    }

    private func loadSettings() async {
        loadingSettings = true
        await model.bootstrap()
        await model.loadSiteConfig()
        loadingSettings = false
    }

    private func sendCode() {
        guard !busy, canValidate, !email.isEmpty, Date() >= resendAfter else { return }
        sendingCode = true
        formError = nil
        Task {
            defer { sendingCode = false; captcha.reset() }
            do {
                let token = try await captcha.resolve(required: captchaRequired, action: "send_email")
                guard !Task.isCancelled else { return }
                await model.sendEmailCode(email.trimmingCharacters(in: .whitespacesAndNewlines), captchaToken: token)
                resendAfter = Date().addingTimeInterval(30)
            } catch { formError = error.localizedDescription }
        }
    }

    private func submit() {
        guard !blocked else { return }
        submitting = true
        formError = nil
        Task {
            defer { submitting = false; captcha.reset() }
            do {
                let token = try await captcha.resolve(required: captchaRequired, action: "reset_password")
                guard !Task.isCancelled else { return }
                await model.forgetPassword(email: email.trimmingCharacters(in: .whitespacesAndNewlines), password: password, code: emailCode, captchaToken: token)
            } catch { formError = error.localizedDescription }
        }
    }
}
