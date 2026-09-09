import SwiftUI
import CoreImage.CIFilterBuiltins

@MainActor
struct OrderPaymentView: View {
    @Bindable var model: AppModel
    let tradeNo: String
    @Environment(\.scenePhase) private var scenePhase
    @State private var order: ServerOrder?
    @State private var methods: [PaymentMethod] = []
    @State private var methodID: Int64?
    @State private var loading = false
    @State private var checking = false
    @State private var submitting = false
    @State private var cancelling = false
    @State private var error: String?
    @State private var redirect: URL?
    @State private var qrContent: String?
    @State private var waitingSince: Date?

    private var method: PaymentMethod? { methods.first { $0.id == methodID } }
    private var amount: Int64 { order?.totalAmount ?? 0 }
    private var quote: PaymentTerms? { order.flatMap { try? paymentTerms($0, method: method) } }
    private var quoteError: String? {
        guard let order, order.status == 0 else { return nil }
        do { _ = try paymentTerms(order, method: method); return nil }
        catch { return error.localizedDescription }
    }
    private var busy: Bool { submitting || loading || checking || cancelling }
    private var paid: Bool { order?.status == 3 || order?.status == 4 }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                SufePanel {
                    Label(paid ? "订阅已生效" : statusText, systemImage: paid ? "checkmark.seal.fill" : "receipt")
                        .font(.title3.bold()).foregroundStyle(paid ? ProtonStyle.green : ProtonStyle.text)
                    Text(tradeNo).font(.caption.monospaced()).foregroundStyle(.secondary).textSelection(.enabled)
                    if let order {
                        Divider()
                        if let discount = order.discountAmount, discount > 0 { LabeledContent("优惠抵扣", value: "−\(formatPriceCents(discount))") }
                        if let balance = order.balanceAmount, balance > 0 { LabeledContent("余额抵扣", value: "−\(formatPriceCents(balance))") }
                        LabeledContent("订单应付", value: formatPriceCents(amount))
                        if let quote {
                            LabeledContent("支付手续费", value: formatPriceCents(quote.fee))
                            LabeledContent("合计", value: formatPriceCents(quote.total)).font(.headline)
                        }
                    } else if loading { ProgressView() }
                }
                if order?.status == 0 {
                    SufePanel {
                        Text(amount == 0 ? "账户已抵扣全部金额" : "选择支付方式").font(.headline)
                        if amount > 0 {
                            if methods.isEmpty { Text("当前没有可用支付渠道，请稍后重试或联系客户支持。").font(.caption).foregroundStyle(.secondary) }
                            ForEach(methods, id: \.id) { item in
                                Button {
                                    methodID = item.id; redirect = nil; qrContent = nil
                                } label: {
                                    HStack {
                                        Image(systemName: methodID == item.id ? "largecircle.fill.circle" : "circle")
                                        Text(item.name).foregroundStyle(.primary)
                                        Spacer()
                                    }.padding(.vertical, 8)
                                }.disabled(busy || (order?.paymentId ?? 0) > 0)
                            }
                        }
                        if let quoteError { Text(quoteError).font(.caption).foregroundStyle(.red) }
                        Button(submitting ? "正在请求支付…" : amount == 0 ? "确认开通" : quote.map { "确认支付 \(formatPriceCents($0.total))" } ?? "暂不可付款") { checkout() }
                            .buttonStyle(.borderedProminent)
                            .disabled(busy || quote == nil)
                        Text("支付结果以服务端订单状态为准。关闭页面后，可在“我的订单”继续处理。")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                if let redirect {
                    SufePanel {
                        Label("支付页面已就绪", systemImage: "creditcard")
                        Link("打开支付页面", destination: redirect).buttonStyle(.borderedProminent)
                        Text("完成后回到客户端，订单状态会自动更新。")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                if let qrContent, let image = makeQRCode(qrContent) {
                    SufePanel {
                        Text("扫码支付").font(.headline)
                        Image(uiImage: image).interpolation(.none).resizable().scaledToFit().frame(maxWidth: 240)
                            .padding(12).background(.white).frame(maxWidth: .infinity)
                    }
                }
                if let error { Text(error).font(.caption).foregroundStyle(.red) }
                HStack {
                    Button(loading || checking ? "查询中…" : "刷新订单状态") { Task { await load() } }.disabled(busy)
                    Spacer()
                    if order?.status == 0 {
                        Button("取消订单", role: .destructive) {
                            cancel()
                        }.disabled(busy)
                    }
                }.font(.callout)
            }.padding(18).frame(maxWidth: 680)
        }.background(ProtonStyle.background)
            .navigationTitle("核对订单")
            .task { await load() }
            .task(id: waitingSince) {
                guard let started = waitingSince else { return }
                while Date().timeIntervalSince(started) < 600 && !Task.isCancelled {
                    do { try await Task.sleep(for: .seconds(5)) } catch { return }
                    guard !paid, order?.status != 2 else { return }
                    if scenePhase == .active { await refresh() }
                }
                if !paid && order?.status != 2 { error = "暂未确认付款。你可以手动刷新，或稍后在订单中查看。" }
            }
            .onChange(of: scenePhase) { _, phase in if phase == .active { Task { await refresh() } } }
    }

    private var statusText: String {
        switch order?.status {
        case 0: return "核对你的订单"
        case 1: return "已支付，正在开通"
        case 2: return "订单已取消"
        case 3, 4: return "订阅已生效"
        default: return "正在读取订单"
        }
    }
    private func load() async {
        guard !busy else { return }
        loading = true; error = nil
        defer { loading = false }
        let generation = model.authGeneration
        do {
            let next = try await model.serverOrder(tradeNo)
            guard generation == model.authGeneration else { return }
            order = next
            if next.status != 0 { redirect = nil; qrContent = nil }
            if next.totalAmount > 0 && next.status == 0 {
                let nextMethods = try await model.requireClient().fetchPaymentMethods()
                guard generation == model.authGeneration else { return }
                methods = nextMethods.filter { $0.id > 0 }
                methodID = selectedMethod(for: next, from: methods, preferred: methodID)
            }
            if next.status == 1 { waitingSince = Date() }
        } catch { if generation == model.authGeneration { self.error = model.friendly(error) } }
    }
    private func refresh(allowDuringMutation: Bool = false) async {
        guard !checking, !loading, allowDuringMutation || (!submitting && !cancelling) else { return }
        checking = true
        defer { checking = false }
        let generation = model.authGeneration
        do {
            let next = try await model.serverOrder(tradeNo)
            guard generation == model.authGeneration else { return }
            let newlyPaid = !paid && (next.status == 3 || next.status == 4)
            order = next
            methodID = selectedMethod(for: next, from: methods, preferred: methodID)
            if next.status != 0 { redirect = nil; qrContent = nil }
            if newlyPaid { redirect = nil; qrContent = nil; await model.refreshHome(); await model.refreshOrders() }
        } catch { if generation == model.authGeneration { self.error = model.friendly(error) } }
    }
    private func checkout() {
        guard order?.status == 0, !busy, let confirmed = quote else { return }
        submitting = true; error = nil
        let generation = model.authGeneration
        Task {
            defer { submitting = false }
            do {
                // Re-fetch both order and channels. Never authorise terms the
                // user has not seen, including a newly bound payment channel.
                let current = try await model.serverOrder(tradeNo)
                guard generation == model.authGeneration else { return }
                guard current.status == 0 else {
                    order = current; redirect = nil; qrContent = nil
                    if current.status == 1 { waitingSince = Date() }
                    return
                }
                let currentMethods: [PaymentMethod]
                if current.totalAmount > 0 {
                    let available = try await model.requireClient().fetchPaymentMethods()
                    currentMethods = available.filter { $0.id > 0 }
                } else { currentMethods = [] }
                guard generation == model.authGeneration else { return }
                let selected = selectedMethod(for: current, from: currentMethods, preferred: confirmed.paymentID)
                order = current
                methods = currentMethods
                methodID = selected
                let currentTerms = try paymentTerms(current, method: currentMethods.first { $0.id == selected })
                guard currentTerms == confirmed else {
                    redirect = nil; qrContent = nil
                    error = "订单金额或支付渠道已更新，请核对后再次确认。"
                    return
                }
                let result = try await model.checkout(tradeNo, methodId: currentTerms.paymentID)
                guard generation == model.authGeneration else { return }
                // checkout binds the fee on the server. Verify the bound
                // amount before revealing an external link or payment QR.
                let settledOrder = try await model.serverOrder(tradeNo)
                guard generation == model.authGeneration else { return }
                order = settledOrder
                methodID = selectedMethod(for: settledOrder, from: currentMethods, preferred: currentTerms.paymentID)
                if settledOrder.status == 2 {
                    redirect = nil; qrContent = nil
                    error = "订单已取消，请查看订单记录。"
                    return
                }
                if settledOrder.status == 1 {
                    redirect = nil; qrContent = nil
                    waitingSince = Date()
                    return
                }
                if settledOrder.status == 3 || settledOrder.status == 4 {
                    redirect = nil; qrContent = nil
                    await model.refreshHome()
                    guard generation == model.authGeneration else { return }
                    await model.refreshOrders()
                    return
                }
                let settledTerms = try paymentTerms(settledOrder, method: currentMethods.first { $0.id == methodID })
                guard settledTerms.amount == confirmed.amount,
                      settledTerms.balance == confirmed.balance,
                      settledTerms.discount == confirmed.discount,
                      settledTerms.paymentID == confirmed.paymentID,
                      settledTerms.fee == confirmed.fee,
                      settledTerms.total == confirmed.total else {
                    redirect = nil; qrContent = nil
                    error = "支付服务更新了实际应付金额，请重新核对后确认。"
                    return
                }
                let payload = paymentPayload(result.dataJson)
                redirect = payload.flatMap(safePaymentURL)
                qrContent = result.kind == 0 ? payload : nil
                waitingSince = Date()
                if result.kind != -1 && redirect == nil && qrContent == nil { error = "支付服务未返回可用链接，请查询订单或联系客户支持。" }
                await refresh(allowDuringMutation: true)
            } catch {
                guard generation == model.authGeneration else { return }
                self.error = "未确认支付请求结果，请先刷新订单状态。\(model.friendly(error))"
                waitingSince = Date()
            }
        }
    }

    private func cancel() {
        guard !busy, order?.status == 0 else { return }
        cancelling = true
        let generation = model.authGeneration
        Task {
            defer { cancelling = false }
            await model.cancelOrder(tradeNo)
            guard generation == model.authGeneration else { return }
            await refresh(allowDuringMutation: true)
            await model.refreshOrders()
        }
    }
}

private struct PaymentTerms: Equatable {
    let tradeNo: String
    let amount: Int64
    let balance: Int64
    let discount: Int64
    let boundPaymentID: Int64?
    let paymentID: Int64
    let paymentName: String
    let paymentDriver: String
    let fee: Int64
    let total: Int64
}

private func selectedMethod(for order: ServerOrder, from methods: [PaymentMethod], preferred: Int64?) -> Int64? {
    if order.totalAmount == 0 { return nil }
    if let bound = order.paymentId, bound > 0 { return bound }
    if let preferred, preferred > 0, methods.contains(where: { $0.id == preferred }) { return preferred }
    return methods.first(where: { $0.id > 0 })?.id
}

private func paymentTerms(_ order: ServerOrder, method: PaymentMethod?) throws -> PaymentTerms {
    guard order.totalAmount >= 0, (order.handlingAmount ?? 0) >= 0 else {
        throw AppError.unavailable("订单金额异常，请联系客户支持。")
    }
    let bound = order.paymentId.flatMap { $0 > 0 ? $0 : nil }
    var fee: Int64 = 0
    if order.totalAmount > 0 && order.status != 0 {
        fee = order.handlingAmount ?? 0
    } else if order.totalAmount > 0 {
        guard let method, method.id > 0, bound == nil || bound == method.id else {
            throw AppError.unavailable(bound == nil ? "请选择可用支付渠道。" : "原支付渠道暂不可用，请稍后重试或联系客户支持。")
        }
        if bound != nil {
            // A bound order retains its original fee, including null/zero.
            fee = order.handlingAmount ?? 0
        } else {
            let rawPercent = method.handlingFeePercent ?? 0
            guard rawPercent.isFinite else { throw AppError.unavailable("支付渠道费率无效，请联系客户支持。") }
            var decimalFee = Decimal(order.totalAmount) * Decimal(min(100, max(0, rawPercent))) / 100 + Decimal(max(0, method.handlingFeeFixed ?? 0))
            var rounded = Decimal()
            NSDecimalRound(&rounded, &decimalFee, 0, .plain)
            guard !rounded.isNaN, rounded >= 0, rounded <= Decimal(Int64.max) else {
                throw AppError.unavailable("支付手续费超出有效范围，请联系客户支持。")
            }
            fee = NSDecimalNumber(decimal: rounded).int64Value
        }
    }
    let (total, overflow) = order.totalAmount.addingReportingOverflow(fee)
    guard !overflow else { throw AppError.unavailable("订单总额超出有效范围，请联系客户支持。") }
    return PaymentTerms(tradeNo: order.tradeNo, amount: order.totalAmount,
        balance: order.balanceAmount ?? 0, discount: order.discountAmount ?? 0,
        boundPaymentID: bound, paymentID: order.totalAmount == 0 ? 0 : method?.id ?? 0,
        paymentName: order.totalAmount == 0 ? "" : method?.name ?? "",
        paymentDriver: order.totalAmount == 0 ? "" : method?.payment ?? "", fee: fee, total: total)
}

private func paymentPayload(_ json: String) -> String? {
    guard let value = try? JSONSerialization.jsonObject(with: Data(json.utf8), options: [.fragmentsAllowed]) else { return nil }
    if let string = value as? String { return string }
    if let object = value as? [String: Any] {
        for key in ["url", "redirect", "qrcode", "qr_code", "qrCode", "code_url"] {
            if let string = object[key] as? String { return string }
        }
    }
    return nil
}
private func safePaymentURL(_ value: String) -> URL? {
    guard let url = URL(string: value), ["https", "http"].contains(url.scheme?.lowercased() ?? ""), url.host != nil, url.user == nil, url.password == nil else { return nil }
    return url
}
private func makeQRCode(_ value: String) -> UIImage? {
    guard value.utf8.count <= 2953 else { return nil }
    let filter = CIFilter.qrCodeGenerator()
    filter.message = Data(value.utf8)
    guard let output = filter.outputImage?.transformed(by: CGAffineTransform(scaleX: 8, y: 8)),
          let image = CIContext().createCGImage(output, from: output.extent) else { return nil }
    return UIImage(cgImage: image)
}
