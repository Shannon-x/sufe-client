import SwiftUI

struct TicketComposerSheet: View {
    @Bindable var model: AppModel
    @Environment(\.dismiss) private var dismiss

    @State private var subject = ""
    @State private var level: Int32 = 0
    @State private var message = ""
    @State private var localError: String?
    @State private var submitting = false

    var body: some View {
        NavigationStack {
            Form {
                Section(String(localized: "tickets.composer.subject")) {
                    TextField(
                        String(localized: "tickets.composer.subject.placeholder"),
                        text: $subject
                    )
                    .disabled(submitting)
                }

                Section(String(localized: "tickets.composer.level")) {
                    Picker("", selection: $level) {
                        Text(String(localized: "tickets.level.low")).tag(Int32(0))
                        Text(String(localized: "tickets.level.normal")).tag(Int32(1))
                        Text(String(localized: "tickets.level.high")).tag(Int32(2))
                    }
                    .pickerStyle(.segmented)
                    .disabled(submitting)
                }

                Section(String(localized: "tickets.composer.message")) {
                    TextField(
                        String(localized: "tickets.composer.message.placeholder"),
                        text: $message,
                        axis: .vertical
                    )
                    .lineLimit(4...10)
                    .disabled(submitting)
                }

                if let err = localError {
                    Section { Text(err).foregroundStyle(.red) }
                }
            }
            .navigationTitle(String(localized: "tickets.composer.new"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(String(localized: "tickets.composer.cancel")) { dismiss() }
                        .disabled(submitting)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(String(localized: "tickets.composer.submit")) { submit() }
                        .disabled(submitting)
                }
            }
        }
    }

    private func submit() {
        let s = subject.trimmingCharacters(in: .whitespacesAndNewlines)
        let m = message.trimmingCharacters(in: .whitespacesAndNewlines)
        if s.isEmpty || m.isEmpty {
            localError = String(localized: "tickets.composer.fill_all")
            return
        }
        submitting = true
        Task {
            await model.saveTicket(SaveTicketArgs(
                subject: s,
                level: level,
                message: m
            ))
            submitting = false
            dismiss()
        }
    }
}
