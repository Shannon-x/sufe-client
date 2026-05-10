import SwiftUI

/// One option a user can pick when subscribing to a plan. Mirrors the
/// Android `PeriodOption`.
struct PeriodOption: Identifiable, Hashable {
    let id = UUID()
    let key: String
    let labelKey: LocalizedStringResource
    let priceCents: Int64
}

func collectPeriods(_ plan: Plan) -> [PeriodOption] {
    var out: [PeriodOption] = []
    if let v = plan.monthPrice    { out.append(.init(key: "month_price",    labelKey: "plans.period.month",    priceCents: v)) }
    if let v = plan.quarterPrice  { out.append(.init(key: "quarter_price",  labelKey: "plans.period.quarter",  priceCents: v)) }
    if let v = plan.halfYearPrice { out.append(.init(key: "half_year_price",labelKey: "plans.period.half_year",priceCents: v)) }
    if let v = plan.yearPrice     { out.append(.init(key: "year_price",     labelKey: "plans.period.year",     priceCents: v)) }
    if let v = plan.twoYearPrice  { out.append(.init(key: "two_year_price", labelKey: "plans.period.two_year", priceCents: v)) }
    if let v = plan.threeYearPrice{ out.append(.init(key: "three_year_price", labelKey: "plans.period.three_year",priceCents: v)) }
    if let v = plan.onetimePrice  { out.append(.init(key: "onetime_price",  labelKey: "plans.period.onetime",  priceCents: v)) }
    if let v = plan.resetPrice    { out.append(.init(key: "reset_price",    labelKey: "plans.period.reset",    priceCents: v)) }
    return out
}

struct PlansView: View {
    @Bindable var model: AppModel
    @State private var purchaseTarget: Plan?

    var body: some View {
        Group {
            if let plans = model.plans {
                if plans.isEmpty {
                    ContentUnavailableView(
                        String(localized: "plans.empty"),
                        systemImage: "tray"
                    )
                } else {
                    List(plans, id: \.id) { plan in
                        PlanCard(plan: plan) {
                            purchaseTarget = plan
                        }
                        .listRowSeparator(.hidden)
                    }
                    .listStyle(.plain)
                }
            } else {
                ProgressView()
            }
        }
        .navigationTitle(String(localized: "tabs.plans"))
        .refreshable { await model.refreshPlans() }
        .task { await model.refreshPlans() }
        .sheet(item: $purchaseTarget) { plan in
            PurchaseSheet(model: model, plan: plan)
        }
    }
}

private struct PlanCard: View {
    let plan: Plan
    let onPurchase: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(plan.name).font(.headline)
                if !plan.sell {
                    Tag(text: String(localized: "plans.tag.not_selling"), color: .secondary)
                }
                if !plan.renew {
                    Tag(text: String(localized: "plans.tag.no_renew"), color: .orange)
                }
            }
            if !plan.content.isEmpty {
                Text(plan.content)
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Text(String(format: NSLocalizedString("plans.transfer.gb", comment: ""), plan.transferEnable))
                .font(.caption)

            ForEach(collectPeriods(plan)) { period in
                HStack {
                    Text(period.labelKey).font(.callout)
                    Spacer()
                    Text(formatPriceCents(period.priceCents))
                        .font(.callout.weight(.semibold))
                }
            }

            Button(action: onPurchase) {
                Text(String(localized: "plans.purchase"))
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .disabled(!plan.sell)
            .padding(.top, 4)
        }
        .padding(16)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 16))
        .padding(.vertical, 4)
    }
}

private struct Tag: View {
    let text: String
    let color: Color
    var body: some View {
        Text(text)
            .font(.caption)
            .padding(.horizontal, 8).padding(.vertical, 2)
            .background(color.opacity(0.15), in: Capsule())
            .foregroundStyle(color)
    }
}
