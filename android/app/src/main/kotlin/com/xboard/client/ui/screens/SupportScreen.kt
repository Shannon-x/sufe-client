package com.xboard.client.ui.screens

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.vm.AppViewModel
import java.text.DateFormat
import java.util.Date

@Composable
fun SupportScreen(viewModel: AppViewModel, onBack: () -> Unit, onTickets: () -> Unit) {
    val state by viewModel.business.state.collectAsState()
    val context = LocalContext.current
    var draft by remember { mutableStateOf("") }
    val list = rememberLazyListState()
    DisposableEffect(Unit) {
        viewModel.business.setChatViewing(true)
        onDispose { viewModel.business.setChatViewing(false) }
    }
    LaunchedEffect(state.messages.lastOrNull()?.id) { if (state.messages.isNotEmpty()) list.animateScrollToItem(state.messages.lastIndex) }
    ScreenScaffold(title = "在线客服", onBack = onBack, actions = {
        TextButton(onClick = { viewModel.business.refreshChat() }, enabled = !state.chatBusy) { Text("刷新") }
    }) { padded ->
        if (!state.enabled("chatwoot")) Column(padded.padding(24.dp), verticalArrangement = Arrangement.spacedBy(18.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Text("在线客服暂未开放", style = MaterialTheme.typography.titleLarge)
            Text("服务方开启客服后，你可以在应用内直接联系支持团队。")
            if (state.enabled("tickets")) Button(onClick = onTickets) { Text("提交服务工单") }
        } else Column(padded.imePadding().padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            if (state.conversations.size > 1) Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                state.conversations.forEach { conversation -> FilterChip(selected = conversation.id == state.conversationId, onClick = { viewModel.business.selectConversation(conversation.id) }, enabled = !state.chatBusy && !state.chatSending, label = { Text("会话 #${conversation.id}${if (conversation.status == "resolved") " · 已解决" else ""}") }) }
            }
            if (state.chatBusy && state.messages.isEmpty()) LinearProgressIndicator(Modifier.fillMaxWidth())
            state.chatError?.let { error ->
                Surface(color = MaterialTheme.colorScheme.errorContainer, shape = RoundedCornerShape(12.dp)) {
                    Column(Modifier.padding(12.dp)) { Text(error); TextButton(onClick = { viewModel.business.refreshChat() }) { Text("重试") } }
                }
            }
            LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, verticalArrangement = Arrangement.spacedBy(12.dp), contentPadding = PaddingValues(vertical = 14.dp)) {
                if (state.messages.isEmpty()) item {
                    BusinessCard("你好，有什么可以帮你？") { Text("描述你遇到的问题，客服回复后会在应用内提醒你。") }
                }
                items(state.messages, key = { it.id }) { message ->
                    Column(Modifier.fillMaxWidth(), horizontalAlignment = if (message.outgoing) Alignment.End else Alignment.Start) {
                        if (!message.outgoing) Text(message.sender, style = MaterialTheme.typography.labelSmall, modifier = Modifier.padding(bottom = 4.dp))
                        Surface(shape = RoundedCornerShape(18.dp), color = if (message.outgoing) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceVariant, modifier = Modifier.widthIn(max = 320.dp)) {
                            Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                if (message.content.isNotBlank()) Text(message.content)
                                message.attachments.forEach { attachment -> TextButton(onClick = {
                                    runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(attachment.url))) }
                                        .onFailure { viewModel.emitClipboardCopy(attachment.url, "附件链接已复制") }
                                }) { Text(attachment.title) } }
                            }
                        }
                        if (message.createdAt > 0) Text(DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(message.createdAt * 1000)), style = MaterialTheme.typography.labelSmall)
                    }
                }
            }
            Row(Modifier.fillMaxWidth().padding(bottom = 12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.Bottom) {
                OutlinedTextField(value = draft, onValueChange = { if (it.length <= 10_000) draft = it }, placeholder = { Text("描述你遇到的问题…") }, enabled = !state.chatSending, modifier = Modifier.weight(1f).heightIn(max = 150.dp), maxLines = 5)
                Button(onClick = { viewModel.business.sendMessage(draft) { draft = "" } }, enabled = draft.isNotBlank() && !state.chatSending && !state.chatBusy) { Text(if (state.chatSending) "发送中" else "发送") }
            }
        }
    }
}
