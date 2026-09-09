import Foundation
import UserNotifications

extension AppModel {
    func refreshClientFeatures(force: Bool = false) async {
        guard !featuresRefreshing, let c = deploymentClient(),
              force || Date().timeIntervalSince(featuresLastRefresh) > 60 else { return }
        featuresRefreshing = true
        defer { featuresRefreshing = false }
        do {
            let next = try decodeCore(MobileClientConfig.self, await c.fetchClientConfigJson())
            let supportChanged = next.chatwootBaseUrl != clientFeatures.chatwootBaseUrl ||
                next.chatwootInboxIdentifier != clientFeatures.chatwootInboxIdentifier
            clientFeatures = next
            featuresLoaded = true
            featuresLastRefresh = Date()
            if !next.enabled("notice") { notices = [] }
            if !next.enabled("purchase") { plans = [] }
            if !next.enabled("tickets") { tickets = []; ticketDetail = nil }
            if !next.enabled("gift_card") { giftHistory = [] }
            if !next.enabled("invite") { inviteSummary = nil }
            if !next.enabled("chatwoot") || supportChanged { resetChat() }
        } catch {
            // Keep the most recent verified configuration on transient failure.
            if !featuresLoaded { snackbar = "暂时无法加载服务配置，请检查网络后重试。" }
        }
    }

    func refreshBenefits() async {
        guard !benefitsBusy, let c = try? requireClient() else { return }
        let generation = authGeneration
        benefitsBusy = true
        benefitsError = nil
        defer { benefitsBusy = false }
        await refreshClientFeatures()
        guard generation == authGeneration else { return }
        do {
            if clientFeatures.enabled("gift_card") {
                let history = try decodeCore(GiftHistory.self, await c.giftCardHistoryJson(page: 1))
                guard generation == authGeneration else { return }
                giftHistory = history.data
            }
            if clientFeatures.enabled("invite") {
                let invites = try decodeCore(InviteSummary.self, await c.fetchInvitesJson())
                guard generation == authGeneration else { return }
                inviteSummary = invites
            }
        } catch { if generation == authGeneration { benefitsError = friendly(error) } }
    }

    func checkGift(_ code: String) async throws -> GiftPreview {
        guard clientFeatures.enabled("gift_card") else { throw AppError.notReady }
        let generation = authGeneration
        let result = try decodeCore(GiftPreview.self, await requireClient().giftCardCheckJson(code: code))
        guard generation == authGeneration else { throw CancellationError() }
        return result
    }

    func redeemGift(_ code: String) async throws -> GiftReceipt {
        guard clientFeatures.enabled("gift_card") else { throw AppError.notReady }
        let generation = authGeneration
        let result = try decodeCore(GiftReceipt.self, await requireClient().giftCardRedeemJson(code: code))
        guard generation == authGeneration else { throw CancellationError() }
        await refreshHome()
        guard generation == authGeneration else { throw CancellationError() }
        await refreshBenefits()
        guard generation == authGeneration else { throw CancellationError() }
        return result
    }

    func generateInvite() async throws {
        guard clientFeatures.enabled("invite") else { throw AppError.notReady }
        let c = try requireClient()
        let generation = authGeneration
        _ = try await c.createInvite(code: nil)
        guard generation == authGeneration else { throw CancellationError() }
        let result = try decodeCore(InviteSummary.self, await c.fetchInvitesJson())
        guard generation == authGeneration else { throw CancellationError() }
        inviteSummary = result
    }

    func refreshCustomRules() async {
        guard let c = try? requireClient() else { return }
        let generation = authGeneration
        do {
            let rules = try decodeCore([MobileRule].self, await c.customRulesJson())
            guard generation == authGeneration else { return }
            customRules = rules
        } catch { if generation == authGeneration { snackbar = friendly(error) } }
    }

    func saveCustomRules(_ rules: [MobileRule]) async throws {
        guard !rulesBusy, clientFeatures.enabled("custom_rules") else { throw AppError.notReady }
        rulesBusy = true
        defer { rulesBusy = false }
        let c = try requireClient()
        let generation = authGeneration
        let json = String(decoding: try JSONEncoder().encode(rules), as: UTF8.self)
        try await c.saveCustomRulesJson(json: json)
        guard generation == authGeneration else { throw CancellationError() }
        customRules = rules
        snackbar = "规则已保存，下次连接时生效。"
    }

    func serverOrder(_ tradeNo: String) async throws -> ServerOrder {
        let generation = authGeneration
        let result = try decodeCore(ServerOrder.self, await requireClient().fetchOrderJson(tradeNo: tradeNo))
        guard generation == authGeneration else { throw CancellationError() }
        guard result.tradeNo == tradeNo else { throw AppError.unavailable("订单返回信息不一致，请重新查询。") }
        return result
    }

    func resetChat() {
        chatGeneration += 1
        chatConversations = []
        chatMessages = []
        chatSelectedID = nil
        chatLastSeen = 0
        chatUnread = 0
        chatBusy = false
        chatSending = false
        chatError = nil
    }

    private var chatMarkerKey: String {
        "sufe.chat.seen:\(session?.email ?? ""):\(clientFeatures.chatwootBaseUrl ?? ""):\(clientFeatures.chatwootInboxIdentifier ?? "")"
    }

    func markChatRead() {
        guard chatViewing else { return }
        chatLastSeen = max(chatLastSeen, chatMessages.map(\.id).max() ?? 0)
        UserDefaults.standard.set(chatLastSeen, forKey: chatMarkerKey)
        chatUnread = 0
    }

    func refreshChat() async {
        guard !chatBusy, clientFeatures.enabled("chatwoot"), let c = try? requireClient() else { return }
        let generation = authGeneration
        let scope = chatMarkerKey
        let chatVersion = chatGeneration
        chatBusy = true
        defer { if generation == authGeneration, chatVersion == chatGeneration { chatBusy = false } }
        do {
            if chatSelectedID == nil {
                let restored = try decodeCore(ChatRestore.self, await c.supportRequestJson(action: "restore", conversationId: nil, content: nil))
                guard generation == authGeneration, scope == chatMarkerKey, chatVersion == chatGeneration else { return }
                chatConversations = restored.conversations.sorted { $0.id > $1.id }
                chatSelectedID = chatConversations.first?.id
                chatLastSeen = (UserDefaults.standard.object(forKey: chatMarkerKey) as? NSNumber)?.int64Value ?? 0
            }
            if let id = chatSelectedID {
                guard id > 0 else { throw AppError.unavailable("客服会话编号无效，请刷新重试。") }
                let rows = try decodeCore([ChatMessage].self, await c.supportRequestJson(action: "messages", conversationId: UInt64(id), content: nil))
                guard generation == authGeneration, scope == chatMarkerKey, chatVersion == chatGeneration else { return }
                let previous = Set(chatMessages.map(\.id))
                let hadPrevious = !chatMessages.isEmpty
                chatMessages = rows.filter { $0.private != true }.sorted { $0.id < $1.id }
                chatUnread = chatMessages.filter { $0.incoming && $0.id > chatLastSeen }.count
                if chatViewing { markChatRead() }
                else if hadPrevious && chatMessages.contains(where: { $0.incoming && $0.id > chatLastSeen && !previous.contains($0.id) }) {
                    await notifyChatReply()
                }
            }
            chatError = nil
        } catch { if generation == authGeneration, scope == chatMarkerKey, chatVersion == chatGeneration { chatError = friendly(error) } }
    }

    func sendChat(_ text: String) async throws {
        guard !chatSending, clientFeatures.enabled("chatwoot") else { throw AppError.notReady }
        let content = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !content.isEmpty, content.count <= 4_000 else { throw AppError.unavailable("消息长度需在 1–4000 字之间。") }
        let generation = authGeneration
        let scope = chatMarkerKey
        let chatVersion = chatGeneration
        let c = try requireClient()
        chatSending = true
        defer { if generation == authGeneration, chatVersion == chatGeneration { chatSending = false } }
        if chatSelectedID == nil {
            let created = try decodeCore(ChatConversation.self, await c.supportRequestJson(action: "start", conversationId: nil, content: nil))
            guard generation == authGeneration, scope == chatMarkerKey, chatVersion == chatGeneration else { throw CancellationError() }
            chatSelectedID = created.id
            chatConversations.insert(created, at: 0)
        }
        guard let id = chatSelectedID, id > 0 else { throw AppError.notReady }
        let message = try decodeCore(ChatMessage.self, await c.supportRequestJson(action: "send", conversationId: UInt64(id), content: content))
        guard generation == authGeneration, scope == chatMarkerKey, chatVersion == chatGeneration else { throw CancellationError() }
        if !chatMessages.contains(where: { $0.id == message.id }) { chatMessages.append(message) }
        markChatRead()
    }

    func requestChatNotifications() async {
        do {
            let granted = try await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound])
            snackbar = granted ? "已开启客服消息提醒。" : "可在系统设置中开启通知。"
        } catch { snackbar = friendly(error) }
    }

    private func notifyChatReply() async {
        let center = UNUserNotificationCenter.current()
        let settings = await center.notificationSettings()
        guard settings.authorizationStatus == .authorized else { return }
        let content = UNMutableNotificationContent()
        content.title = "客户支持有新回复"
        content.body = "打开 Sufe 查看客服消息。"
        content.sound = .default
        try? await center.add(UNNotificationRequest(identifier: "sufe.chat.reply", content: content, trigger: nil))
    }
}
