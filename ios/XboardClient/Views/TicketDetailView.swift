import SwiftUI

struct TicketDetailView: View {
    @Bindable var model: AppModel
    let ticketId: Int64

    @State private var reply = ""
    @State private var localError: String?
    @State private var sending = false
    @State private var showCloseConfirm = false

    var body: some View {
        ScrollView {
            if let detail = model.ticketDetail, detail.id == ticketId {
                content(detail: detail)
            } else {
                ProgressView().padding(40)
            }
        }
        .navigationTitle(String(localized: "tickets.detail.title"))
        .navigationBarTitleDisplayMode(.inline)
        .task { await model.openTicket(id: ticketId) }
        .alert(
            String(localized: "tickets.close"),
            isPresented: $showCloseConfirm
        ) {
            Button(String(localized: "tickets.confirm.no"), role: .cancel) {}
            Button(String(localized: "tickets.confirm.yes"), role: .destructive) {
                Task { await model.closeTicket(id: ticketId) }
            }
        } message: {
            Text(String(localized: "tickets.confirm.close"))
        }
    }

    @ViewBuilder
    private func content(detail: TicketDetail) -> some View {
        VStack(spacing: 12) {
            // header card
            VStack(alignment: .leading, spacing: 6) {
                Text(detail.subject)
                    .font(.headline)
                HStack(spacing: 8) {
                    LevelChip(level: detail.level)
                    TicketStatusChip(status: detail.status)
                }
                if let created = detail.createdAt {
                    Text("\(String(localized: "tickets.created_at")): \(formatDateTime(created))")
                        .font(.caption2).foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(16)
            .background(Color(.secondarySystemBackground),
                        in: RoundedRectangle(cornerRadius: 16))

            // messages
            if detail.message.isEmpty {
                Text(String(localized: "tickets.no_messages"))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                ForEach(detail.message, id: \.id) { msg in
                    MessageBubble(msg: msg)
                }
            }

            // reply or closed banner
            if detail.status == 0 {
                replyBox(detail: detail)
            } else {
                VStack(alignment: .leading, spacing: 4) {
                    Text(String(localized: "tickets.closed"))
                        .font(.subheadline.weight(.semibold))
                    Text(String(localized: "tickets.closed.hint"))
                        .font(.caption).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(12)
                .background(Color(.secondarySystemBackground),
                            in: RoundedRectangle(cornerRadius: 12))
            }
        }
        .screenPadding()
    }

    @ViewBuilder
    private func replyBox(detail: TicketDetail) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            TextField(
                String(localized: "tickets.reply.placeholder"),
                text: $reply,
                axis: .vertical
            )
            .textFieldStyle(.roundedBorder)
            .lineLimit(3...6)
            .disabled(sending)

            if let err = localError {
                Text(err).font(.footnote).foregroundStyle(.red)
            }

            HStack(spacing: 8) {
                Button(String(localized: "tickets.close"), role: .destructive) {
                    showCloseConfirm = true
                }
                .buttonStyle(.bordered)
                .disabled(sending)
                .frame(maxWidth: .infinity)

                Button {
                    let trimmed = reply.trimmingCharacters(in: .whitespacesAndNewlines)
                    if trimmed.isEmpty {
                        localError = String(localized: "tickets.reply.empty")
                        return
                    }
                    sending = true
                    Task {
                        await model.replyTicket(id: detail.id, message: trimmed)
                        reply = ""
                        sending = false
                    }
                } label: {
                    if sending {
                        ProgressView().tint(.white)
                    } else {
                        Text(String(localized: "tickets.reply.send"))
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(sending)
                .frame(maxWidth: .infinity)
            }
        }
    }
}

private struct MessageBubble: View {
    let msg: TicketMessage

    var body: some View {
        HStack {
            if msg.isMe { Spacer(minLength: 40) }
            VStack(alignment: .leading, spacing: 4) {
                Text(msg.message)
                    .font(.callout)
                if let created = msg.createdAt {
                    Text(formatDateTime(created))
                        .font(.caption2).foregroundStyle(.secondary)
                }
            }
            .padding(12)
            .frame(maxWidth: 320, alignment: .leading)
            .background(
                msg.isMe
                    ? Color.accentColor.opacity(0.18)
                    : Color(.tertiarySystemFill),
                in: RoundedRectangle(cornerRadius: 12)
            )
            if !msg.isMe { Spacer(minLength: 40) }
        }
    }
}
