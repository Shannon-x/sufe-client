import SwiftUI

struct TicketsView: View {
    @Bindable var model: AppModel
    @State private var composerOpen = false
    @State private var openedTicketId: Int64?

    var body: some View {
        Group {
            if let tickets = model.tickets {
                if tickets.isEmpty {
                    ContentUnavailableView(
                        String(localized: "tickets.empty"),
                        systemImage: "envelope"
                    )
                } else {
                    List(tickets, id: \.id) { ticket in
                        Button {
                            openedTicketId = ticket.id
                        } label: {
                            TicketRow(ticket: ticket)
                        }
                        .buttonStyle(.plain)
                        .listRowSeparator(.hidden)
                    }
                    .listStyle(.plain)
                }
            } else {
                ProgressView()
            }
        }
        .navigationTitle(String(localized: "tickets.title"))
        .refreshable { await model.refreshTickets() }
        .task { await model.refreshTickets() }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    composerOpen = true
                } label: {
                    Image(systemName: "plus")
                }
            }
        }
        .sheet(isPresented: $composerOpen) {
            TicketComposerSheet(model: model)
        }
        .navigationDestination(item: $openedTicketId) { id in
            TicketDetailView(model: model, ticketId: id)
        }
    }
}

private struct TicketRow: View {
    let ticket: Ticket

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(ticket.subject)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(2)
                Spacer()
                LevelChip(level: ticket.level)
            }
            HStack {
                StatusChip(status: ticket.status)
                Spacer()
                if let updated = ticket.updatedAt {
                    Text(formatDateTime(updated))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(16)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 16))
        .padding(.vertical, 4)
    }
}

struct LevelChip: View {
    let level: Int32

    var body: some View {
        let (text, color) = palette()
        Text(text)
            .font(.caption)
            .padding(.horizontal, 8).padding(.vertical, 2)
            .background(color.opacity(0.15), in: Capsule())
            .foregroundStyle(color)
    }

    private func palette() -> (String, Color) {
        switch level {
        case 0: return (String(localized: "tickets.level.low"),    .gray)
        case 1: return (String(localized: "tickets.level.normal"), .blue)
        default: return (String(localized: "tickets.level.high"),  .red)
        }
    }
}

struct TicketStatusChip: View {
    let status: Int32

    var body: some View {
        let (text, color) = palette()
        Text(text)
            .font(.caption)
            .padding(.horizontal, 8).padding(.vertical, 2)
            .background(color.opacity(0.15), in: Capsule())
            .foregroundStyle(color)
    }

    private func palette() -> (String, Color) {
        if status == 0 {
            return (String(localized: "tickets.status.open"), .orange)
        } else {
            return (String(localized: "tickets.status.closed"), .gray)
        }
    }
}

private struct StatusChip: View {
    let status: Int32
    var body: some View { TicketStatusChip(status: status) }
}
