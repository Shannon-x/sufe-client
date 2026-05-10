import SwiftUI

private enum PurchasePhase: Equatable {
    case form
    case submitting
    case awaitingGateway(tradeNo: String, redirect: String?)
    case balancePaid(tradeNo: String)
    case status(tradeNo: String, statusInt: Int32?)
}

struct PurchaseSheet: View {
    @Bindable var model: AppModel
    let plan: Plan
    @Environment(\.dismiss) private var dismiss

    @State private var period: PeriodOption?
    @State private var coupon = ""
    @State private var methodId: Int64?
    @State private var phase: PurchasePhase = .form
    @State private var localError: String?

    var body: some View {
        NavigationStack {
            Group {
                switch phase {
                case .form:
                    formSection
                case .submitting:
                    ProgressView()
                case let .awaitingGateway(tradeNo, redirect):
                    gatewaySection(tradeNo: tradeNo, redirect: redirect)
                case let .balancePaid(tradeNo):
                    paidSection(tradeNo: tradeNo)
                case let .status(tradeNo, statusInt):
                    statusSection(tradeNo: tradeNo, statusInt: statusInt)
                }
            }
            .navigationTitle(plan.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(String(localized: "common.close")) { dismiss() }
                }
            }
            .task {
                await model.refreshPlans()
                if period == nil { period = collectPeriods(plan).first }
                if methodId == nil { methodId = model.paymentMethods?.first?.id }
            }
        }
    }

    // ---------- form ----------

    private var formSection: some View {
        Form {
            Section(String(localized: "purchase.section.period")) {
                ForEach(collectPeriods(plan)) { p in
                    HStack {
                        Image(systemName: period?.id == p.id ? "largecircle.fill.circle" : "circle")
                            .foregroundStyle(.tint)
                        Text(p.labelKey)
                        Spacer()
                        Text(formatPriceCents(p.priceCents))
                            .foregroundStyle(.secondary)
                    }
                    .contentShape(Rectangle())
                    .onTapGesture { period = p }
                }
            }

            Section(String(localized: "purchase.section.coupon")) {
                TextField(String(localized: "purchase.coupon.placeholder"), text: $coupon)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }

            Section(String(localized: "purchase.section.payment")) {
                if let methods = model.paymentMethods {
                    ForEach(methods, id: \.id) { method in
                        PaymentMethodRow(method: method, isSelected: methodId == method.id) {
                            methodId = method.id
                        }
                    }
                } else {
                    ProgressView()
                }
            }

            if let err = localError {
                Section { Text(err).foregroundStyle(.red) }
            }

            Section {
                Button(action: submit) {
                    Text(String(localized: "purchase.submit"))
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .disabled(period == nil || methodId == nil)
            }
        }
    }

    // ---------- gateway / balance / status ----------

    private func gatewaySection(tradeNo: String, redirect: String?) -> some View {
        VStack(spacing: 16) {
            Text(String(localized: "purchase.gateway.title"))
                .font(.headline)
            if let url = redirect, let parsed = URL(string: url) {
                Link(destination: parsed) {
                    Text(String(localized: "purchase.gateway.open"))
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                Text(url).font(.caption).foregroundStyle(.secondary)
                    .textSelection(.enabled)
            } else {
                Text(String(localized: "purchase.gateway.no_redirect"))
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            HStack {
                Button(String(localized: "purchase.gateway.refresh")) {
                    Task { await refreshStatus(tradeNo: tradeNo) }
                }
                Spacer()
                Button(String(localized: "purchase.gateway.cancel")) {
                    Task {
                        await model.cancelOrder(tradeNo)
                        phase = .form
                    }
                }
            }
            Spacer()
        }
        .screenPadding()
    }

    private func paidSection(tradeNo: String) -> some View {
        VStack(spacing: 16) {
            Image(systemName: "checkmark.seal.fill")
                .font(.system(size: 56))
                .foregroundStyle(.green)
            Text(String(localized: "purchase.paid.title"))
                .font(.headline)
            Text(tradeNo).font(.caption).foregroundStyle(.secondary)
            Button(String(localized: "common.ok")) { dismiss() }
                .buttonStyle(.borderedProminent)
        }
        .screenPadding()
    }

    private func statusSection(tradeNo: String, statusInt: Int32?) -> some View {
        VStack(spacing: 16) {
            ProgressView()
            Text(statusInt.map { statusText($0) } ?? String(localized: "purchase.status.checking"))
            Button(String(localized: "purchase.gateway.refresh")) {
                Task { await refreshStatus(tradeNo: tradeNo) }
            }
            Button(String(localized: "common.close")) { dismiss() }
        }
        .screenPadding()
    }

    // ---------- actions ----------

    private func submit() {
        guard let p = period, let m = methodId else { return }
        localError = nil
        phase = .submitting
        Task {
            do {
                let tradeNo = try await model.saveOrder(SaveOrderArgs(
                    planId: plan.id,
                    period: p.key,
                    couponCode: coupon.isEmpty ? nil : coupon
                ))
                let resp = try await model.checkout(tradeNo, methodId: m)
                switch resp.kind {
                case 1:
                    phase = .awaitingGateway(tradeNo: tradeNo, redirect: extractURL(resp.dataJson))
                case 0:
                    phase = .awaitingGateway(tradeNo: tradeNo, redirect: extractURL(resp.dataJson))
                case -2:
                    phase = .awaitingGateway(tradeNo: tradeNo, redirect: nil)
                default:
                    phase = .balancePaid(tradeNo: tradeNo)
                }
            } catch {
                localError = String(describing: error)
                phase = .form
            }
        }
    }

    private func refreshStatus(tradeNo: String) async {
        do {
            let s = try await model.checkOrderStatus(tradeNo)
            if s == 2 || s == 3 {
                phase = .balancePaid(tradeNo: tradeNo)
            } else {
                phase = .status(tradeNo: tradeNo, statusInt: s)
            }
        } catch {
            localError = String(describing: error)
        }
    }

    private func statusText(_ s: Int32) -> String {
        switch s {
        case 0: return String(localized: "purchase.status.pending")
        case 1: return String(localized: "purchase.status.activating")
        case 2: return String(localized: "purchase.status.cancelled")
        case 3: return String(localized: "purchase.status.completed")
        case 4: return String(localized: "purchase.status.discounted")
        default: return "?"
        }
    }

    /// `dataJson` may be a raw URL string, or `{ "url": "...", ... }` /
    /// `{ "qr_code": "..." }` — pick whichever shape contains an http(s) URL.
    private func extractURL(_ json: String) -> String? {
        if json.hasPrefix("\"") {
            // raw-string-encoded URL
            return json.dropFirst().dropLast().description
        }
        guard let data = json.data(using: .utf8),
              let parsed = try? JSONSerialization.jsonObject(with: data) else {
            return nil
        }
        if let s = parsed as? String { return s }
        if let dict = parsed as? [String: Any] {
            for key in ["url", "redirect", "qr_code"] {
                if let v = dict[key] as? String { return v }
            }
        }
        return nil
    }
}

private struct PaymentMethodRow: View {
    let method: PaymentMethod
    let isSelected: Bool
    let onTap: () -> Void

    var body: some View {
        HStack {
            Image(systemName: isSelected ? "largecircle.fill.circle" : "circle")
                .foregroundStyle(.tint)
            VStack(alignment: .leading, spacing: 2) {
                Text(method.name)
                if let fee = feeLabel {
                    Text(fee).font(.caption).foregroundStyle(.secondary)
                }
            }
            Spacer()
        }
        .contentShape(Rectangle())
        .onTapGesture { onTap() }
    }

    private var feeLabel: String? {
        var pieces: [String] = []
        if let f = method.handlingFeeFixed, f > 0 {
            pieces.append(String(format: NSLocalizedString("purchase.payment.fee_fixed", comment: ""), formatPriceCents(f)))
        }
        if let p = method.handlingFeePercent, p > 0 {
            pieces.append(String(format: NSLocalizedString("purchase.payment.fee_percent", comment: ""), p))
        }
        return pieces.isEmpty ? nil : pieces.joined(separator: "  ")
    }
}
