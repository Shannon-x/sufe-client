import SwiftUI

@MainActor
struct AccountView: View {
    @Bindable var model: AppModel
    @AppStorage("sufe.autoConnect") private var autoConnect = false
    @State private var code = ""
    @State private var checkedCode = ""
    @State private var preview: GiftPreview?
    @State private var receipt: GiftReceipt?
    @State private var working = false
    @State private var inviting = false
    @State private var error: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                SufePanel {
                    HStack(spacing: 14) {
                        Image(systemName: "person.crop.circle.fill")
                            .font(.system(size: 44)).foregroundStyle(ProtonStyle.accent)
                        VStack(alignment: .leading, spacing: 4) {
                            Text("我的 Sufe").font(.title3.bold())
                            Text(model.session?.email ?? "").font(.caption).foregroundStyle(.secondary)
                                .textSelection(.enabled)
                        }
                    }
                    Divider().padding(.vertical, 8)
                    HStack {
                        balance("账户余额", cents: model.user?.balance ?? 0)
                        Spacer()
                        balance("推广佣金", cents: model.user?.commissionBalance ?? 0)
                    }
                }
                if model.clientFeatures.enabled("gift_card") { giftCard }
                if model.clientFeatures.enabled("invite") { inviteCard }
                SufePanel {
                    Text("偏好与工具").font(.headline)
                    Toggle("登录后自动连接", isOn: $autoConnect)
                    Text("开启后，登录成功且订阅有效时会请求连接 VPN。")
                        .font(.caption).foregroundStyle(.secondary)
                    Divider()
                    if model.clientFeatures.enabled("custom_rules") {
                        NavigationLink { CustomRulesView(model: model) } label: {
                            Label("自定义分流规则", systemImage: "arrow.triangle.branch")
                        }.padding(.vertical, 7)
                    }
                    NavigationLink { OrdersView(model: model) } label: {
                        Label("我的订单", systemImage: "receipt")
                    }.padding(.vertical, 7)
                    if model.clientFeatures.enabled("chatwoot") {
                        Button { Task { await model.requestChatNotifications() } } label: {
                            Label("开启客服消息通知", systemImage: "bell.badge")
                        }.padding(.vertical, 7)
                    }
                    Button { Task { await model.refreshClientFeatures(force: true); await model.refreshBenefits() } } label: {
                        Label("刷新服务配置", systemImage: "arrow.clockwise")
                    }.padding(.vertical, 7)
                    Divider()
                    Button(role: .destructive) { Task { await model.logout() } } label: {
                        Label("退出登录", systemImage: "rectangle.portrait.and.arrow.right")
                    }.padding(.top, 7)
                }
                if let error = model.benefitsError { Text(error).font(.caption).foregroundStyle(.red) }
            }
            .padding(18).frame(maxWidth: 680)
        }
        .background(ProtonStyle.background)
        .navigationTitle("我的账户")
        .refreshable { await model.refreshHome(); await model.refreshBenefits() }
        .task { await model.refreshBenefits() }
    }

    private func balance(_ title: String, cents: Int64) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.caption).foregroundStyle(.secondary)
            Text(formatPriceCents(cents)).font(.title3.bold()).minimumScaleFactor(0.65).lineLimit(1)
        }
    }

    private var giftCard: some View {
        SufePanel {
            Label("兑换一份好心情", systemImage: "gift").font(.headline)
            Text("输入礼品卡，先查看奖励，再确认兑换。")
                .font(.caption).foregroundStyle(.secondary)
            TextField("礼品卡兑换码", text: $code)
                .textInputAutocapitalization(.never).autocorrectionDisabled()
                .textFieldStyle(.roundedBorder).disabled(working)
                .onChange(of: code) { _, _ in preview = nil }
            Button(working ? "处理中…" : "查看奖励") { checkGift() }
                .buttonStyle(.borderedProminent).disabled(working || code.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            if let preview {
                Divider()
                Text(preview.name.isEmpty ? "礼品卡奖励" : preview.name).font(.subheadline.bold())
                if preview.mystery {
                    Text("盲盒随机发放，以下为预览，最终奖励以兑换结果为准。")
                        .font(.caption).foregroundStyle(.secondary)
                }
                ForEach(preview.rewards, id: \.self) { key in
                    LabeledContent(rewardLabel(key), value: rewardText(key, preview.rewardPreview[key] ?? .null, preview: preview))
                        .font(.subheadline)
                }
                if let reason = preview.reason { Text(reason).font(.caption).foregroundStyle(.secondary) }
                if preview.canRedeem {
                    Button("确认兑换") { redeemGift() }.buttonStyle(.borderedProminent).disabled(working)
                }
            }
            if let receipt {
                Label(receipt.message, systemImage: "checkmark.seal.fill").foregroundStyle(ProtonStyle.green)
                ForEach(receipt.rewards.keys.sorted().filter { !["random_rewards", "weight"].contains($0) }, id: \.self) { key in
                    LabeledContent(rewardLabel(key), value: rewardText(key, receipt.rewards[key] ?? .null)).font(.caption)
                }
            }
            if let error { Text(error).font(.caption).foregroundStyle(.red) }
            Divider().padding(.vertical, 6)
            Text("最近兑换").font(.subheadline.bold())
            if model.giftHistory.isEmpty { Text("还没有兑换记录。期待你的第一份礼物。").font(.caption).foregroundStyle(.secondary) }
            ForEach(model.giftHistory.prefix(5)) { entry in
                LabeledContent(entry.templateName, value: entry.date).font(.caption)
            }
        }
    }

    private var inviteCard: some View {
        SufePanel {
            Label("好连接，值得分享", systemImage: "person.2").font(.headline)
            Text("把邀请码分享给朋友，奖励以账户规则为准。")
                .font(.caption).foregroundStyle(.secondary)
            if let invites = model.inviteSummary {
                HStack {
                    VStack { Text(invites.value(0).formatted()).bold(); Text("已邀请").font(.caption) }
                    Spacer()
                    VStack { Text(Int64(exactly: invites.value(1)).map { formatPriceCents($0) } ?? "金额异常").bold(); Text("有效佣金").font(.caption) }
                    Spacer()
                    VStack { Text("\(invites.value(3).formatted())%").bold(); Text("返佣比例").font(.caption) }
                }.padding(.vertical, 10)
                ForEach(invites.codes) { invite in
                    HStack {
                        Text(invite.code).font(.system(.body, design: .monospaced)).textSelection(.enabled)
                        Spacer()
                        ShareLink(item: invite.code) { Image(systemName: "square.and.arrow.up") }
                            .accessibilityLabel("分享邀请码")
                    }.padding(12).background(ProtonStyle.accent.opacity(0.07), in: RoundedRectangle(cornerRadius: 10))
                }
            }
            Button(inviting ? "生成中…" : "生成邀请码") {
                inviting = true
                Task {
                    defer { inviting = false }
                    do { try await model.generateInvite() }
                    catch { model.snackbar = model.friendly(error) }
                }
            }.buttonStyle(.bordered).disabled(inviting)
        }
    }

    private func checkGift() {
        working = true; error = nil; preview = nil; receipt = nil
        let value = code.trimmingCharacters(in: .whitespacesAndNewlines)
        Task {
            defer { working = false }
            do { preview = try await model.checkGift(value); checkedCode = value }
            catch { self.error = model.friendly(error) }
        }
    }
    private func redeemGift() {
        guard preview?.canRedeem == true, checkedCode == code.trimmingCharacters(in: .whitespacesAndNewlines) else { return }
        working = true; error = nil
        Task {
            defer { working = false }
            do {
                let result = try await model.redeemGift(checkedCode)
                code = ""; preview = nil; receipt = result
            } catch { self.error = model.friendly(error) }
        }
    }
}

struct SufePanel<Content: View>: View {
    let content: Content
    init(@ViewBuilder content: () -> Content) { self.content = content() }
    var body: some View {
        VStack(alignment: .leading, spacing: 13) { content }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(20)
            .background(ProtonStyle.panel, in: RoundedRectangle(cornerRadius: 20))
            .overlay(RoundedRectangle(cornerRadius: 20).stroke(ProtonStyle.panelBorder))
    }
}
