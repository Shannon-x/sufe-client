import SwiftUI

/// Creating an order and authorising its payment are separate, reviewable steps.
@MainActor
struct PurchaseSheet: View {
    @Bindable var model: AppModel
    let plan: Plan
    @Environment(\.dismiss) private var dismiss
    @State private var period: PeriodOption?
    @State private var coupon = ""
    @State private var working = false
    @State private var error: String?
    @State private var tradeNo: String?
    @State private var uncertain = false

    var body: some View {
        NavigationStack {
            Group {
                if let tradeNo {
                    OrderPaymentView(model: model, tradeNo: tradeNo)
                } else if !model.clientFeatures.enabled("purchase") {
                    ContentUnavailableView("订阅购买暂未开放", systemImage: "creditcard")
                } else {
                    Form {
                        Section("选择订阅周期") {
                            ForEach(collectPeriods(plan, currentPlanId: model.subscribe?.planId)) { option in
                                Button {
                                    period = option
                                } label: {
                                    HStack {
                                        Image(systemName: period?.id == option.id ? "largecircle.fill.circle" : "circle")
                                        Text(option.labelKey).foregroundStyle(.primary)
                                        Spacer()
                                        Text(formatPriceCents(option.priceCents)).foregroundStyle(.secondary)
                                    }
                                }.disabled(working || uncertain)
                            }
                        }
                        Section("优惠码（选填）") {
                            TextField("输入优惠码", text: $coupon).textInputAutocapitalization(.never)
                                .autocorrectionDisabled().disabled(working || uncertain)
                        }
                        Section {
                            Text("下一步会生成订单，展示优惠、余额抵扣和服务端实际应付金额，再由你确认付款。")
                                .font(.caption).foregroundStyle(.secondary)
                            if let error { Text(error).font(.caption).foregroundStyle(.red) }
                            if uncertain {
                                NavigationLink("前往订单检查结果") { OrdersView(model: model) }
                                Text("这次请求的结果尚未确认，请先检查订单，避免重复下单。")
                                    .font(.caption).foregroundStyle(.secondary)
                            } else {
                                Button(working ? "正在生成订单…" : "下一步 · 核对订单") { createOrder() }
                                    .disabled(period == nil || working)
                            }
                        }
                    }
                }
            }
            .navigationTitle(plan.name).navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("关闭") { dismiss() }.disabled(working) } }
            .onAppear { if period == nil { period = collectPeriods(plan, currentPlanId: model.subscribe?.planId).first } }
        }
    }

    private func createOrder() {
        guard let period, !working, !uncertain else { return }
        working = true; error = nil
        let generation = model.authGeneration
        Task {
            defer { working = false }
            do {
                let result = try await model.saveOrder(SaveOrderArgs(planId: plan.id, period: period.key, couponCode: coupon.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : coupon.trimmingCharacters(in: .whitespacesAndNewlines)))
                guard generation == model.authGeneration else { return }
                tradeNo = result
            } catch {
                guard generation == model.authGeneration else { return }
                self.error = model.friendly(error)
                // The request may have reached the server. Retrying a POST is
                // deliberately not automatic, even when the gateway timed out.
                if let ffi = error as? FfiError {
                    switch ffi {
                    case .ApiFailure, .Unauthorized, .Config: uncertain = false
                    default: uncertain = true
                    }
                } else { uncertain = true }
            }
        }
    }
}
