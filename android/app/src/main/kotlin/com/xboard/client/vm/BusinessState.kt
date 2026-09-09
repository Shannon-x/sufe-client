package com.xboard.client.vm

import org.json.JSONArray
import org.json.JSONObject

val featureLabels = linkedMapOf(
    "purchase" to "订阅购买", "gift_card" to "礼品卡", "invite" to "邀请好友",
    "tickets" to "服务工单", "notice" to "公告", "custom_rules" to "自定义规则", "chatwoot" to "在线客服",
)

data class BusinessConfig(
    val brand: String = "Sufe",
    val features: Map<String, Boolean> = emptyMap(),
    val source: String = "",
) {
    companion object {
        fun parse(json: String): BusinessConfig {
            val obj = JSONObject(json)
            val flags = obj.optJSONObject("features") ?: JSONObject()
            return BusinessConfig(obj.optString("brand_name", "Sufe"), featureLabels.keys.associateWith { flags.optBoolean(it, false) }, obj.optString("config_source"))
        }
    }
}

data class GiftPreview(val code: String, val title: String, val canRedeem: Boolean, val reason: String, val rewards: List<Pair<String, String>>, val mystery: Boolean)
data class GiftRecord(val id: Long, val title: String, val date: String, val rewards: List<Pair<String, String>>)
data class InviteCode(val id: Long, val code: String)
data class CustomRule(val id: String, val kind: String, val value: String, val target: String, val enabled: Boolean = true) {
    fun json() = JSONObject().put("id", id).put("kind", kind).put("value", value).put("target", target).put("enabled", enabled)
}
data class ChatAttachment(val title: String, val url: String)
data class ChatMessage(val id: Long, val content: String, val outgoing: Boolean, val sender: String, val createdAt: Long, val attachments: List<ChatAttachment>)
data class ChatConversation(val id: Long, val status: String)
data class BusinessState(
    val config: BusinessConfig? = null,
    val hidden: Set<String> = emptySet(),
    val configError: String? = null,
    val configLoading: Boolean = false,
    val gift: GiftPreview? = null,
    val giftHistory: List<GiftRecord> = emptyList(),
    val giftBusy: Boolean = false,
    val giftError: String? = null,
    val inviteCodes: List<InviteCode> = emptyList(),
    val inviteStats: List<Long> = emptyList(),
    val inviteBusy: Boolean = false,
    val inviteError: String? = null,
    val rules: List<CustomRule> = emptyList(),
    val rulesLoaded: Boolean = false,
    val rulesBusy: Boolean = false,
    val rulesError: String? = null,
    val conversations: List<ChatConversation> = emptyList(),
    val conversationId: Long? = null,
    val messages: List<ChatMessage> = emptyList(),
    val chatBusy: Boolean = false,
    val chatSending: Boolean = false,
    val chatError: String? = null,
    val unread: Int = 0,
) {
    fun enabled(feature: String) = config?.features?.get(feature) == true && feature !in hidden
}

internal fun JSONArray.objects(): List<JSONObject> = (0 until length()).mapNotNull { optJSONObject(it) }
internal fun JSONObject.text(key: String): String = if (isNull(key)) "" else optString(key)
internal fun JSONObject.rewards(): List<Pair<String, String>> {
    val labels = mapOf("balance" to "账户余额", "commission_balance" to "推广余额", "transfer_enable" to "流量额度", "traffic" to "流量", "days" to "有效天数", "expire_days" to "延长有效期", "plan_id" to "套餐编号", "reset_traffic" to "重置流量", "reset_package" to "重置已用流量", "device_limit" to "设备数量", "invite_reward_rate" to "邀请奖励比例")
    return keys().asSequence().filter { it != "random_rewards" && it != "weight" }.map { key ->
        val value = opt(key)
        val rendered = when {
            key.contains("balance") && value is Number -> "¥%.2f".format(value.toDouble() / 100)
            key in listOf("traffic", "transfer_enable") && value is Number -> "%.2f GB".format(value.toDouble() / (1024 * 1024 * 1024))
            key in listOf("days", "expire_days") -> "$value 天"
            key in listOf("reset_traffic", "reset_package") -> if (value == true || value == 1) "是" else "否"
            else -> value?.toString().orEmpty()
        }
        (labels[key] ?: key) to rendered
    }.toList()
}

internal fun JSONObject.chatMessage(): ChatMessage? {
    if (optBoolean("private") || optInt("message_type", -1) !in 0..1) return null
    return ChatMessage(optLong("id"), text("content"), optInt("message_type") == 0,
        optJSONObject("sender")?.text("name")?.ifBlank { "客服" } ?: "客服", optLong("created_at"),
        optJSONArray("attachments")?.objects()?.mapNotNull { attachment ->
            val url = attachment.text("data_url")
            if (!url.startsWith("https://")) null else ChatAttachment(attachment.text("fallback_title").ifBlank { "查看附件" }, url)
        } ?: emptyList())
}
