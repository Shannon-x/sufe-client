package com.xboard.client.vm

import android.content.SharedPreferences
import com.xboard.client.core.Client
import com.xboard.client.core.FfiException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

/** Native account, routing and public Chatwoot UI backed by the shared core.
 * No API URL, Chatwoot access token or contact source ID is accepted from UI. */
class BusinessController(
    private val clientProvider: () -> Client,
    private val scope: CoroutineScope,
    private val preferences: SharedPreferences,
    private val notify: (String) -> Unit,
    private val refreshAccount: () -> Unit,
    private val onExpired: () -> Unit,
) {
    private val client get() = clientProvider()
    private val _state = MutableStateFlow(BusinessState(hidden = preferences.getStringSet("hidden_features", emptySet())!!.toSet()))
    val state = _state.asStateFlow()
    private var account: String? = null
    private var epoch = 0L
    private var featureEpoch = 0L
    private var configJob: Job? = null
    private var chatJob: Job? = null
    private var foreground = true
    private var viewingChat = false
    private val seenMessages = mutableSetOf<Long>()
    private var chatInitialized = false
    private var chatRequest = 0L
    private var sendRequest = 0L
    private fun current(version: Long) = version == epoch && account != null
    private fun allowed(feature: String, version: Long, flags: Long) = current(version) && featureEpoch == flags && _state.value.enabled(feature)

    fun startSession(email: String) {
        if (account == email) return
        stopSession()
        account = email
        val version = epoch
        configJob = scope.launch {
            while (current(version)) {
                if (foreground) refreshConfigNow()
                delay(60_000)
            }
        }
    }

    fun stopSession() {
        epoch++
        featureEpoch++
        account = null
        configJob?.cancel(); configJob = null
        chatJob?.cancel(); chatJob = null
        seenMessages.clear(); chatInitialized = false; viewingChat = false
        _state.value = BusinessState(hidden = _state.value.hidden)
    }

    fun setForeground(value: Boolean) { foreground = value; if (value && account != null) refreshConfig() }
    fun refreshConfig() { scope.launch { refreshConfigNow() } }
    private suspend fun refreshConfigNow() {
        if (account == null || _state.value.configLoading) return
        val version = epoch
        _state.update { it.copy(configLoading = true) }
        try {
            val config = BusinessConfig.parse(client.fetchClientConfigJson())
            if (!current(version)) return
            val firstConfig = _state.value.config == null
            if (_state.value.config?.features != config.features) featureEpoch++
            _state.update { it.copy(config = config, configLoading = false, configError = null,
                gift = if (config.features["gift_card"] == true) it.gift else null) }
            syncChat()
            if (firstConfig) refreshAccount()
        } catch (error: Exception) {
            if (error is CancellationException) throw error
                if (current(version) && error is FfiException.Unauthorized) onExpired()
            if (current(version)) _state.update { it.copy(configLoading = false, configError = error.message ?: "配置加载失败，请重试") }
        }
    }

    fun setFeature(feature: String, enabled: Boolean) {
        if (_state.value.config?.features?.get(feature) != true) return
        featureEpoch++
        val hidden = _state.value.hidden.toMutableSet().apply { if (enabled) remove(feature) else add(feature) }.toSet()
        preferences.edit().putStringSet("hidden_features", hidden).apply()
        _state.update { it.copy(hidden = hidden, gift = null) }
        syncChat()
    }

    fun clearGift() { _state.update { it.copy(gift = null, giftError = null) } }
    fun checkGift(code: String) {
        if (!_state.value.enabled("gift_card") || _state.value.giftBusy || code.isBlank()) return
        val version = epoch; val flags = featureEpoch; val requested = code.trim()
        _state.update { it.copy(giftBusy = true, gift = null, giftError = null) }
        scope.launch {
            try {
                val obj = JSONObject(client.giftCardCheckJson(requested))
                if (!allowed("gift_card", version, flags)) return@launch
                val template = obj.optJSONObject("code_info")?.optJSONObject("template")
                _state.update { it.copy(gift = GiftPreview(requested, template?.text("name")?.ifBlank { "礼品卡奖励" } ?: "礼品卡奖励", obj.optBoolean("can_redeem"), obj.text("reason"), obj.optJSONObject("reward_preview")?.rewards() ?: emptyList(), template?.optInt("type") == 3)) }
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                if (current(version) && error is FfiException.Unauthorized) onExpired()
                if (current(version)) _state.update { it.copy(giftError = error.message ?: "无法核验礼品卡") }
            } finally { if (current(version)) _state.update { it.copy(giftBusy = false) } }
        }
    }

    fun redeemGift() {
        val gift = _state.value.gift ?: return
        if (!_state.value.enabled("gift_card") || !gift.canRedeem || _state.value.giftBusy) return
        val version = epoch; val flags = featureEpoch
        _state.update { it.copy(giftBusy = true, giftError = null) }
        scope.launch {
            try {
                val result = JSONObject(client.giftCardRedeemJson(gift.code))
                if (!allowed("gift_card", version, flags)) return@launch
                _state.update { it.copy(gift = null) }
                notify(result.text("message").ifBlank { "兑换成功，奖励已计入账户" })
                refreshAccount(); loadBenefits()
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                if (current(version) && error is FfiException.Unauthorized) onExpired()
                if (current(version)) _state.update { it.copy(giftError = error.message ?: "兑换未完成，请重试") }
            } finally { if (current(version)) _state.update { it.copy(giftBusy = false) } }
        }
    }

    fun loadBenefits() {
        val version = epoch; val flags = featureEpoch
        if (_state.value.enabled("gift_card")) scope.launch {
            try {
                val result = JSONObject(client.giftCardHistoryJson(1))
                val records = result.optJSONArray("data")?.objects()?.map { GiftRecord(it.optLong("id"), it.text("template_name"), it.text("created_at"), it.optJSONObject("rewards_given")?.rewards() ?: emptyList()) } ?: emptyList()
                if (allowed("gift_card", version, flags)) _state.update { it.copy(giftHistory = records, giftError = null) }
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                if (current(version) && error is FfiException.Unauthorized) onExpired()
                if (current(version)) _state.update { it.copy(giftError = error.message ?: "兑换记录加载失败") }
            }
        }
        if (_state.value.enabled("invite")) scope.launch { loadInvites(version, flags) }
    }

    private suspend fun loadInvites(version: Long, flags: Long) {
        try {
            val result = JSONObject(client.fetchInvitesJson())
            val codes = result.optJSONArray("codes")?.objects()?.map { InviteCode(it.optLong("id"), it.text("code")) } ?: emptyList()
            val stat = result.optJSONArray("stat") ?: JSONArray()
            if (allowed("invite", version, flags)) _state.update { it.copy(inviteCodes = codes, inviteStats = (0 until stat.length()).map { i -> stat.optLong(i) }, inviteError = null) }
        } catch (error: Exception) {
            if (error is CancellationException) throw error
                if (current(version) && error is FfiException.Unauthorized) onExpired()
            if (current(version)) _state.update { it.copy(inviteError = error.message ?: "邀请信息加载失败") }
        }
    }

    fun createInvite() {
        if (!_state.value.enabled("invite") || _state.value.inviteBusy) return
        val version = epoch; val flags = featureEpoch
        _state.update { it.copy(inviteBusy = true) }
        scope.launch {
            try {
                client.createInvite(null)
                if (!allowed("invite", version, flags)) return@launch
                loadInvites(version, flags); notify("邀请码已生成")
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                if (current(version) && error is FfiException.Unauthorized) onExpired()
                if (current(version)) _state.update { it.copy(inviteError = error.message ?: "生成邀请码失败") }
            } finally { if (current(version)) _state.update { it.copy(inviteBusy = false) } }
        }
    }

    fun loadRules() {
        if (!_state.value.enabled("custom_rules") || _state.value.rulesBusy) return
        val version = epoch; val flags = featureEpoch
        _state.update { it.copy(rulesBusy = true) }
        scope.launch {
            try {
                val rules = JSONArray(client.customRulesJson()).objects().map { CustomRule(it.text("id"), it.text("kind"), it.text("value"), it.text("target"), it.optBoolean("enabled", true)) }
                if (allowed("custom_rules", version, flags)) _state.update { it.copy(rules = rules, rulesLoaded = true, rulesError = null) }
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                if (current(version) && error is FfiException.Unauthorized) onExpired()
                if (current(version)) _state.update { it.copy(rulesError = error.message ?: "规则读取失败") }
            } finally { if (current(version)) _state.update { it.copy(rulesBusy = false) } }
        }
    }

    fun saveRules(rules: List<CustomRule>, onDone: () -> Unit = {}) {
        if (!_state.value.enabled("custom_rules") || !_state.value.rulesLoaded || _state.value.rulesBusy) return
        val version = epoch; val flags = featureEpoch
        _state.update { it.copy(rulesBusy = true, rulesError = null) }
        scope.launch {
            try {
                client.saveCustomRulesJson(JSONArray().apply { rules.forEach { put(it.json()) } }.toString())
                if (allowed("custom_rules", version, flags)) {
                    _state.update { it.copy(rules = rules) }
                    notify("规则已保存，下一次连接时生效")
                    onDone()
                }
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                if (current(version) && error is FfiException.Unauthorized) onExpired()
                if (current(version)) _state.update { it.copy(rulesError = error.message ?: "规则未保存") }
            } finally { if (current(version)) _state.update { it.copy(rulesBusy = false) } }
        }
    }

    private fun syncChat() {
        if (!_state.value.enabled("chatwoot")) {
            chatRequest++; sendRequest++
            chatJob?.cancel(); chatJob = null
            seenMessages.clear(); chatInitialized = false
            _state.update { it.copy(conversations = emptyList(), conversationId = null, messages = emptyList(), unread = 0, chatBusy = false, chatSending = false, chatError = null) }
            return
        }
        if (chatJob?.isActive == true) return
        val version = epoch
        chatJob = scope.launch {
            while (current(version) && _state.value.enabled("chatwoot")) {
                if (foreground) refreshChatNow()
                delay(10_000)
            }
        }
    }
    fun setChatViewing(value: Boolean) {
        viewingChat = value
        if (value) { _state.update { it.copy(unread = 0) }; refreshChat() }
    }
    fun refreshChat() { scope.launch { refreshChatNow() } }
    private suspend fun refreshChatNow() {
        if (!_state.value.enabled("chatwoot") || _state.value.chatBusy || _state.value.chatSending) return
        val version = epoch; val flags = featureEpoch
        val request = ++chatRequest
        _state.update { it.copy(chatBusy = true) }
        try {
            val restored = JSONObject(client.supportRequestJson("restore", null, null))
            if (!allowed("chatwoot", version, flags)) return
            val raw = restored.optJSONArray("conversations")?.objects() ?: emptyList()
            val conversations = raw.map { ChatConversation(it.optLong("id"), it.text("status")) }.sortedByDescending { it.id }
            val selected = _state.value.conversationId?.takeIf { id -> conversations.any { it.id == id } } ?: conversations.firstOrNull()?.id
            val messages = if (selected != null) JSONArray(client.supportRequestJson("messages", selected.toULong(), null)).objects().mapNotNull { it.chatMessage() }.sortedBy { it.createdAt } else emptyList()
            if (!allowed("chatwoot", version, flags)) return
            val latest = (raw.flatMap { it.optJSONArray("messages")?.objects() ?: emptyList() }.mapNotNull { it.chatMessage() } + messages).distinctBy { it.id }
            val incoming = latest.filter { !it.outgoing && it.id !in seenMessages }
            if (chatInitialized && !viewingChat && incoming.isNotEmpty()) {
                _state.update { it.copy(unread = it.unread + incoming.size) }
                notify("客服有 ${incoming.size} 条新消息，请前往在线客服查看")
            }
            seenMessages.addAll(latest.map { it.id }); chatInitialized = true
            _state.update { it.copy(conversations = conversations, conversationId = selected, messages = messages, chatError = null) }
        } catch (error: Exception) {
            if (error is CancellationException) throw error
                if (current(version) && error is FfiException.Unauthorized) onExpired()
            if (current(version)) _state.update { it.copy(chatError = error.message ?: "客服暂时无法连接，请重试") }
        } finally { if (current(version) && request == chatRequest) _state.update { it.copy(chatBusy = false) } }
    }

    fun selectConversation(id: Long) {
        if (_state.value.chatBusy || _state.value.chatSending) return
        _state.update { it.copy(conversationId = id, messages = emptyList()) }
        refreshChat()
    }
    fun sendMessage(content: String, onSent: () -> Unit) {
        val text = content.trim()
        if (!_state.value.enabled("chatwoot") || _state.value.chatSending || _state.value.chatBusy || text.isBlank() || text.length > 10_000) return
        val version = epoch; val flags = featureEpoch
        val request = ++sendRequest
        _state.update { it.copy(chatSending = true, chatError = null) }
        scope.launch {
            try {
                val previous = _state.value.conversationId
                val conversation = previous ?: JSONObject(client.supportRequestJson("start", null, null)).optLong("id")
                if (conversation <= 0 || !allowed("chatwoot", version, flags)) return@launch
                if (previous == null) _state.update { it.copy(conversationId = conversation) }
                val message = JSONObject(client.supportRequestJson("send", conversation.toULong(), text)).chatMessage()
                if (!allowed("chatwoot", version, flags)) return@launch
                if (message != null) _state.update { it.copy(messages = (it.messages + message).distinctBy { msg -> msg.id }) }
                onSent()
            } catch (error: Exception) {
                if (error is CancellationException) throw error
                if (current(version) && error is FfiException.Unauthorized) onExpired()
                if (current(version)) _state.update { it.copy(chatError = error.message ?: "消息未发送，请重试") }
            } finally {
                if (current(version) && request == sendRequest) {
                    _state.update { it.copy(chatSending = false) }
                    refreshChat()
                }
            }
        }
    }
}
