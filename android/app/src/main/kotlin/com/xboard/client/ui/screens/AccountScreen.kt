package com.xboard.client.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.util.formatYuan
import com.xboard.client.vm.AppViewModel
import com.xboard.client.vm.featureLabels

@Composable
fun AccountScreen(viewModel: AppViewModel, onBack: () -> Unit, onOrders: () -> Unit, onRules: () -> Unit, onFeatures: () -> Unit) {
    val state by viewModel.business.state.collectAsState()
    val home by viewModel.home.collectAsState()
    var code by remember { mutableStateOf("") }
    var confirm by remember { mutableStateOf(false) }
    LaunchedEffect(state.enabled("gift_card"), state.enabled("invite")) { viewModel.business.loadBenefits() }
    LaunchedEffect(state.gift) { if (state.gift == null) confirm = false }
    ScreenScaffold(title = "我的账户", onBack = onBack, actions = {
        TextButton(onClick = { viewModel.refreshHome(); viewModel.business.loadBenefits() }) { Text("刷新") }
    }) { padded ->
        ScrollableColumn(padded, verticalArrangement = Arrangement.spacedBy(18.dp)) {
            BusinessCard("我的服务") {
                TextButton(onClick = onOrders) { Text("历史订单与继续付款") }
                if (state.enabled("custom_rules")) TextButton(onClick = onRules) { Text("自定义分流规则") }
                TextButton(onClick = onFeatures) { Text("功能偏好") }
            }
            BusinessCard("账户余额") {
                Text(home.user?.email.orEmpty(), style = MaterialTheme.typography.bodyMedium)
                Text("¥${formatYuan(home.user?.balance ?: 0L)}", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold)
                Text("购买套餐时自动抵扣", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (state.enabled("gift_card")) BusinessCard("兑换一份好心情") {
                Text("输入礼品卡兑换码，先查看奖励，再确认兑换。")
                OutlinedTextField(value = code, onValueChange = { code = it; viewModel.business.clearGift() }, label = { Text("礼品卡兑换码") }, enabled = !state.giftBusy, singleLine = true, modifier = Modifier.fillMaxWidth())
                Button(onClick = { viewModel.business.checkGift(code) }, enabled = !state.giftBusy && code.isNotBlank(), modifier = Modifier.fillMaxWidth()) { Text(if (state.giftBusy) "处理中…" else "查看奖励") }
                state.giftError?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                state.gift?.let { gift ->
                    HorizontalDivider()
                    Text(gift.title, fontWeight = FontWeight.Bold)
                    if (gift.mystery) Text("盲盒奖励随机发放，最终以兑换结果为准。")
                    if (gift.reason.isNotBlank()) Text(gift.reason)
                    gift.rewards.forEach { RewardLine(it.first, it.second) }
                    if (gift.canRedeem) FilledTonalButton(onClick = { confirm = true }, enabled = !state.giftBusy && gift.code == code.trim()) { Text("确认兑换") }
                }
                HorizontalDivider()
                Text("最近兑换", style = MaterialTheme.typography.titleSmall)
                if (state.giftHistory.isEmpty()) Text("还没有兑换记录", color = MaterialTheme.colorScheme.onSurfaceVariant)
                state.giftHistory.take(10).forEach { record ->
                    Text(record.title, fontWeight = FontWeight.SemiBold)
                    if (record.date.isNotBlank()) Text(record.date, style = MaterialTheme.typography.labelSmall)
                    record.rewards.forEach { RewardLine(it.first, it.second) }
                }
            }
            if (state.enabled("invite")) BusinessCard("好连接，值得分享") {
                Text("邀请好友：${state.inviteStats.getOrElse(0) { 0 }} 人")
                Text("有效佣金：¥${formatYuan(state.inviteStats.getOrElse(1) { 0 })}")
                Text("返佣比例：${state.inviteStats.getOrElse(3) { 0 }}%")
                state.inviteError?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                state.inviteCodes.forEach { invite ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text(invite.code, Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                        TextButton(onClick = { viewModel.emitClipboardCopy(invite.code, "邀请码已复制") }) { Text("复制") }
                    }
                }
                Button(onClick = { viewModel.business.createInvite() }, enabled = !state.inviteBusy, modifier = Modifier.fillMaxWidth()) { Text(if (state.inviteBusy) "生成中…" else "生成邀请码") }
            }
            if (!state.enabled("gift_card") && !state.enabled("invite")) BusinessCard("更多权益，敬请期待") { Text("开放的账户权益会自动出现在这里。") }
        }
    }
    if (confirm && state.gift != null && state.enabled("gift_card")) AlertDialog(
        onDismissRequest = { confirm = false }, title = { Text("确认兑换礼品卡") },
        text = { Text("兑换成功后，奖励将计入当前账户。${if (state.gift?.mystery == true) "盲盒奖励以实际发放为准。" else ""}") },
        confirmButton = { Button(onClick = { confirm = false; viewModel.business.redeemGift() }) { Text("确认兑换") } },
        dismissButton = { TextButton(onClick = { confirm = false }) { Text("再想想") } },
    )
}

@Composable
internal fun BusinessCard(title: String, content: @Composable ColumnScope.() -> Unit) {
    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(24.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
            content()
        }
    }
}
@Composable
private fun RewardLine(label: String, value: String) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        Text(label, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
        Text(value, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
fun FeaturesScreen(viewModel: AppViewModel, onBack: () -> Unit) {
    val state by viewModel.business.state.collectAsState()
    ScreenScaffold(title = "功能偏好", onBack = onBack, actions = { TextButton(onClick = { viewModel.business.refreshConfig() }) { Text("刷新") } }) { padded ->
        ScrollableColumn(padded, verticalArrangement = Arrangement.spacedBy(18.dp)) {
            BusinessCard("让客户端更合你的习惯") {
                Text("你可以隐藏暂时不用的功能。服务方关闭的功能会保持关闭；已有订单仍可查看和处理。")
                if (state.configLoading) LinearProgressIndicator(Modifier.fillMaxWidth())
                state.configError?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                featureLabels.forEach { (key, title) ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Column(Modifier.weight(1f)) {
                            Text(title)
                            if (state.config?.features?.get(key) != true) Text("服务方暂未开放", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        Switch(checked = state.enabled(key), onCheckedChange = { viewModel.business.setFeature(key, it) }, enabled = state.config?.features?.get(key) == true)
                    }
                }
            }
        }
    }
}
