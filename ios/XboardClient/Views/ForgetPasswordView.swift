import SwiftUI

struct ForgetPasswordView: View {
    @Bindable var model: AppModel
    @State private var email = ""
    @State private var emailCode = ""
    @State private var password = ""
    @State private var sendingCode = false

    var body: some View {
        ScrollView {
            VStack(spacing: 16) {
                TextField(String(localized: "auth.email"), text: $email)
                    .textFieldStyle(.roundedBorder)
                    .keyboardType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()

                HStack {
                    TextField(String(localized: "auth.email_code"), text: $emailCode)
                        .textFieldStyle(.roundedBorder)
                        .keyboardType(.numberPad)
                    Button(action: sendCode) {
                        if sendingCode { ProgressView() }
                        else { Text(String(localized: "auth.email_code.send")) }
                    }
                    .disabled(email.isEmpty || sendingCode)
                }

                SecureField(String(localized: "auth.password.new"), text: $password)
                    .textFieldStyle(.roundedBorder)

                if let err = model.loginError {
                    Text(err).font(.footnote).foregroundStyle(.red)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }

                Button {
                    Task {
                        await model.forgetPassword(email: email, password: password, code: emailCode)
                    }
                } label: {
                    if model.isAuthBusy { ProgressView().tint(.white) }
                    else {
                        Text(String(localized: "auth.forget.submit"))
                            .frame(maxWidth: .infinity)
                    }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(email.isEmpty || password.isEmpty || emailCode.isEmpty || model.isAuthBusy)
            }
            .screenPadding()
        }
        .navigationTitle(String(localized: "auth.forget.title"))
        .navigationBarTitleDisplayMode(.inline)
    }

    private func sendCode() {
        sendingCode = true
        Task {
            await model.sendEmailCode(email)
            sendingCode = false
        }
    }
}
