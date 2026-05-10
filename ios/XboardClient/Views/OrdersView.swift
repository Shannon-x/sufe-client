import SwiftUI

struct OrdersView: View {
    @Bindable var model: AppModel

    var body: some View {
        Group {
            if let orders = model.orders {
                if orders.isEmpty {
                    ContentUnavailableView(
                        String(localized: "orders.empty"),
                        systemImage: "cart"
                    )
                } else {
                    List(orders, id: \.id) { order in
                        OrderCard(order: order) {
                            Task {
                                await model.cancelOrder(order.tradeNo)
                                await model.refreshOrders()
                            }
                        }
                        .listRowSeparator(.hidden)
                    }
                    .listStyle(.plain)
                }
            } else {
                ProgressView()
            }
        }
        .navigationTitle(String(localized: "orders.title"))
        .refreshable { await model.refreshOrders() }
        .task { await model.refreshOrders() }
    }
}

private struct OrderCard: View {
    let order: Order
    let onCancel: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(order.tradeNo)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer()
                StatusChip(status: order.status)
            }
            if let kind = order.kind {
                MetaRow(label: String(localized: "orders.col.kind"), value: kindLabel(kind))
            }
            if let period = order.period {
                MetaRow(label: String(localized: "orders.col.period"), value: periodLabel(period))
            }
            MetaRow(
                label: String(localized: "orders.col.amount"),
                value: formatPriceCents(order.totalAmount)
            )
            if let created = order.createdAt {
                MetaRow(
                    label: String(localized: "orders.col.created_at"),
                    value: formatDateTime(created)
                )
            }
            if order.status == 0 {
                HStack {
                    Spacer()
                    Button(String(localized: "orders.action.cancel"), role: .destructive,
                           action: onCancel)
                        .buttonStyle(.bordered)
                }
            }
        }
        .padding(16)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 16))
        .padding(.vertical, 4)
    }

    private func kindLabel(_ kind: Int32) -> String {
        switch kind {
        case 1: return String(localized: "orders.kind.new")
        case 2: return String(localized: "orders.kind.renew")
        case 3: return String(localized: "orders.kind.upgrade")
        case 4: return String(localized: "orders.kind.reset")
        default: return String(localized: "orders.kind.new")
        }
    }

    private func periodLabel(_ period: String) -> String {
        switch period {
        case "month_price":      return String(localized: "plans.period.month")
        case "quarter_price":    return String(localized: "plans.period.quarter")
        case "half_year_price":  return String(localized: "plans.period.half_year")
        case "year_price":       return String(localized: "plans.period.year")
        case "two_year_price":   return String(localized: "plans.period.two_year")
        case "three_year_price": return String(localized: "plans.period.three_year")
        case "onetime_price":    return String(localized: "plans.period.onetime")
        case "reset_price":      return String(localized: "plans.period.reset")
        default:                 return period
        }
    }
}

private struct MetaRow: View {
    let label: String
    let value: String
    var body: some View {
        HStack {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
            Spacer()
            Text(value).font(.callout)
        }
    }
}

private struct StatusChip: View {
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
        switch status {
        case 0: return (String(localized: "orders.status.pending"),    .orange)
        case 1: return (String(localized: "orders.status.activating"), .blue)
        case 2: return (String(localized: "orders.status.cancelled"),  .red)
        case 3: return (String(localized: "orders.status.completed"),  .green)
        case 4: return (String(localized: "orders.status.discounted"), .green)
        default: return (String(localized: "orders.status.pending"),   .gray)
        }
    }
}
