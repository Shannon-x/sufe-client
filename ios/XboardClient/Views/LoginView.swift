import SwiftUI

struct LoginView: View {
    @Bindable var model: AppModel
    @State private var email = ""
    @State private var password = ""
    @State private var showRegister = false
    @State private var showForget = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 16) {
                Spacer(minLength: 32)

                Text(String(localized: "auth.login.title"))
                    .font(.largeTitle.bold())

                Text(String(localized: "auth.login.subtitle"))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)

                VStack(spacing: 12) {
                    TextField(String(localized: "auth.email"), text: $email)
                        .textFieldStyle(.roundedBorder)
                        .keyboardType(.emailAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()

                    SecureField(String(localized: "auth.password"), text: $password)
                        .textFieldStyle(.roundedBorder)
                }
                .padding(.top, 16)

                if let err = model.loginError {
                    Text(err)
                        .font(.footnote)
                        .foregroundStyle(.red)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }

                Button {
                    Task { await model.login(email: email, password: password) }
                } label: {
                    if model.isAuthBusy {
                        ProgressView().tint(.white)
                    } else {
                        Text(String(localized: "auth.login.submit"))
                            .frame(maxWidth: .infinity)
                    }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(email.isEmpty || password.isEmpty || model.isAuthBusy)

                HStack {
                    Button(String(localized: "auth.forget")) { showForget = true }
                    Spacer()
                    Button(String(localized: "auth.register.cta")) { showRegister = true }
                }
                .font(.footnote)
                .padding(.top, 4)

                Spacer()
            }
            .screenPadding()
            .task { await model.loadSiteConfig() }
            .navigationDestination(isPresented: $showRegister) {
                RegisterView(model: model)
            }
            .navigationDestination(isPresented: $showForget) {
                ForgetPasswordView(model: model)
            }
        }
    }
}
