package com.xboard.client.ui.screens

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.vm.AppViewModel
import com.xboard.client.vm.CustomRule
import java.util.UUID

private val ruleKinds = linkedMapOf("DOMAIN-SUFFIX" to "域名及子域名", "DOMAIN" to "完整域名", "DOMAIN-KEYWORD" to "域名关键词", "IP-CIDR" to "IPv4 网段", "IP-CIDR6" to "IPv6 网段")
private val ruleTargets = linkedMapOf("PROXY" to "代理", "DIRECT" to "直连", "REJECT" to "拦截")

@Composable
fun RulesScreen(viewModel: AppViewModel, onBack: () -> Unit) {
    val state by viewModel.business.state.collectAsState()
    var editing by remember { mutableStateOf<CustomRule?>(null) }
    var deleting by remember { mutableStateOf<CustomRule?>(null) }
    LaunchedEffect(Unit) { viewModel.business.loadRules() }
    ScreenScaffold(title = "自定义分流", onBack = onBack, actions = {
        TextButton(onClick = { editing = CustomRule(UUID.randomUUID().toString(), "DOMAIN-SUFFIX", "", "PROXY") }, enabled = state.rulesLoaded && !state.rulesBusy && state.rules.size < 200) { Text("添加") }
    }) { padded ->
        ScrollableColumn(padded, verticalArrangement = Arrangement.spacedBy(16.dp)) {
            BusinessCard("连接方式，由你定义") {
                Text("自定义规则优先于订阅规则，按顺序匹配。保存后，下次连接时生效。")
                Text("${state.rules.count { it.enabled }} 条已启用 · 最多 200 条")
                if (state.rulesBusy) LinearProgressIndicator(Modifier.fillMaxWidth())
                state.rulesError?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                if (!state.rulesLoaded) TextButton(onClick = { viewModel.business.loadRules() }) { Text("重新加载") }
            }
            if (state.rulesLoaded && state.rules.isEmpty()) BusinessCard("添加你的第一条规则") { Text("例如：将 example.com 及其子域名设为直接连接。") }
            state.rules.forEachIndexed { index, rule ->
                BusinessCard(rule.value) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text("${ruleKinds[rule.kind] ?: rule.kind} · ${ruleTargets[rule.target] ?: rule.target}", Modifier.weight(1f))
                        Switch(checked = rule.enabled, enabled = !state.rulesBusy, onCheckedChange = { enabled -> viewModel.business.saveRules(state.rules.map { if (it.id == rule.id) it.copy(enabled = enabled) else it }) })
                    }
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TextButton(onClick = { editing = rule }, enabled = !state.rulesBusy) { Text("编辑") }
                        TextButton(onClick = {
                            val next = state.rules.toMutableList(); next[index] = next[index - 1]; next[index - 1] = rule
                            viewModel.business.saveRules(next)
                        }, enabled = !state.rulesBusy && index > 0) { Text("上移") }
                        TextButton(onClick = { deleting = rule }, enabled = !state.rulesBusy) { Text("删除") }
                    }
                }
            }
        }
    }
    editing?.let { rule -> RuleEditor(rule, state.rulesBusy, state.rulesError, onDismiss = { editing = null }, onSave = { next ->
        val existing = state.rules.any { it.id == next.id }
        viewModel.business.saveRules(if (existing) state.rules.map { if (it.id == next.id) next else it } else state.rules + next) { editing = null }
    }) }
    deleting?.let { rule -> AlertDialog(
        onDismissRequest = { deleting = null }, title = { Text("删除这条规则？") }, text = { Text(rule.value) },
        confirmButton = { TextButton(onClick = { deleting = null; viewModel.business.saveRules(state.rules.filter { it.id != rule.id }) }) { Text("删除") } },
        dismissButton = { TextButton(onClick = { deleting = null }) { Text("保留") } },
    ) }
}

@Composable
private fun RuleEditor(rule: CustomRule, busy: Boolean, serverError: String?, onDismiss: () -> Unit, onSave: (CustomRule) -> Unit) {
    var kind by remember(rule.id) { mutableStateOf(rule.kind) }
    var value by remember(rule.id) { mutableStateOf(rule.value) }
    var target by remember(rule.id) { mutableStateOf(rule.target) }
    var error by remember(rule.id) { mutableStateOf<String?>(null) }
    AlertDialog(onDismissRequest = { if (!busy) onDismiss() }, title = { Text(if (rule.value.isBlank()) "添加分流规则" else "编辑分流规则") }, text = {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ruleKinds.forEach { (key, label) -> FilterChip(selected = kind == key, onClick = { kind = key }, enabled = !busy, label = { Text(label) }) }
            }
            OutlinedTextField(value = value, onValueChange = { value = it; error = null }, enabled = !busy, singleLine = true, label = { Text(if (kind.startsWith("IP-CIDR")) "网段，例如 192.168.0.0/16" else "域名，例如 example.com") })
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ruleTargets.forEach { (key, label) -> FilterChip(selected = target == key, onClick = { target = key }, enabled = !busy, label = { Text(label) }) }
            }
            (error ?: serverError)?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        }
    }, confirmButton = { Button(enabled = !busy, onClick = {
        val text = value.trim()
        if (text.isBlank() || text.length > 253 || text.any { it.isWhitespace() || it == ',' } || text.contains("://")) error = "请输入有效域名或网段，不要包含网址协议、空格或逗号。"
        else onSave(rule.copy(kind = kind, value = text, target = target))
    }) { Text(if (busy) "保存中…" else "保存") } }, dismissButton = { TextButton(onClick = onDismiss, enabled = !busy) { Text("取消") } })
}
