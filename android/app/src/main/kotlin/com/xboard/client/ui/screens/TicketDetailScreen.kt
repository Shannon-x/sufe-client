package com.xboard.client.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.xboard.client.R
import com.xboard.client.core.TicketMessage
import com.xboard.client.ui.components.LoadingState
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.ui.components.TopProgressStrip
import com.xboard.client.util.formatDateTime
import com.xboard.client.vm.AppViewModel

@Composable
fun TicketDetailScreen(
    viewModel: AppViewModel,
    ticketId: Long,
    onBack: () -> Unit,
) {
    val state by viewModel.ticketDetail.collectAsState()
    var reply by remember { mutableStateOf("") }
    var localError by remember { mutableStateOf<String?>(null) }
    var sending by remember { mutableStateOf(false) }
    var showCloseConfirm by remember { mutableStateOf(false) }
    val ctx = LocalContext.current

    LaunchedEffect(ticketId) { viewModel.openTicket(ticketId) }

    ScreenScaffold(title = stringResource(R.string.tickets_detail_title), onBack = onBack) { padded ->
        Column(modifier = padded.fillMaxSize()) {
            TopProgressStrip(visible = state.refreshing)
            val detail = state.detail
            if (detail == null) {
                LoadingState(modifier = Modifier.fillMaxSize())
                return@Column
            }

            ScrollableColumn(
                modifier = Modifier.fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Text(
                            text = detail.subject,
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            LevelChip(level = detail.level)
                            StatusChip(status = detail.status)
                        }
                        detail.createdAt?.let {
                            Text(
                                text = "${stringResource(R.string.tickets_created_at)}: ${formatDateTime(it)}",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }

                if (detail.message.isEmpty()) {
                    Text(
                        text = stringResource(R.string.tickets_no_messages),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    for (msg in detail.message) {
                        MessageBubble(msg = msg)
                    }
                }

                if (detail.status == 0) {
                    OutlinedTextField(
                        value = reply,
                        onValueChange = {
                            reply = it
                            localError = null
                        },
                        placeholder = { Text(stringResource(R.string.tickets_reply_placeholder)) },
                        modifier = Modifier.fillMaxWidth(),
                        minLines = 3,
                        enabled = !sending,
                    )
                    if (localError != null) {
                        Text(
                            text = localError!!,
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        OutlinedButton(
                            onClick = { showCloseConfirm = true },
                            modifier = Modifier.weight(1f),
                            enabled = !sending,
                        ) {
                            Text(stringResource(R.string.tickets_close))
                        }
                        Button(
                            onClick = {
                                if (reply.isBlank()) {
                                    localError = ctx.getString(R.string.tickets_reply_empty)
                                    return@Button
                                }
                                sending = true
                                viewModel.replyTicket(detail.id, reply.trim()) {
                                    reply = ""
                                    sending = false
                                }
                            },
                            enabled = !sending,
                            modifier = Modifier.weight(1f),
                        ) {
                            if (sending) {
                                CircularProgressIndicator(
                                    strokeWidth = 2.dp,
                                    modifier = Modifier.padding(2.dp),
                                    color = MaterialTheme.colorScheme.onPrimary,
                                )
                            } else {
                                Text(stringResource(R.string.tickets_reply_send))
                            }
                        }
                    }
                } else {
                    Card(modifier = Modifier.fillMaxWidth()) {
                        Column(modifier = Modifier.padding(12.dp)) {
                            Text(
                                stringResource(R.string.tickets_closed),
                                style = MaterialTheme.typography.titleSmall,
                                fontWeight = FontWeight.SemiBold,
                            )
                            Text(
                                stringResource(R.string.tickets_closed_hint),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }
        }
    }

    if (showCloseConfirm) {
        AlertDialog(
            onDismissRequest = { showCloseConfirm = false },
            title = { Text(stringResource(R.string.tickets_close)) },
            text = { Text(stringResource(R.string.tickets_confirm_close)) },
            confirmButton = {
                TextButton(onClick = {
                    showCloseConfirm = false
                    viewModel.closeTicket(ticketId) {}
                }) {
                    Text(stringResource(R.string.tickets_confirm_yes))
                }
            },
            dismissButton = {
                TextButton(onClick = { showCloseConfirm = false }) {
                    Text(stringResource(R.string.tickets_confirm_no))
                }
            },
        )
    }
}

@Composable
private fun MessageBubble(msg: TicketMessage) {
    val isMe = msg.isMe
    val container =
        if (isMe) MaterialTheme.colorScheme.primaryContainer
        else MaterialTheme.colorScheme.surfaceVariant
    val onContainer =
        if (isMe) MaterialTheme.colorScheme.onPrimaryContainer
        else MaterialTheme.colorScheme.onSurfaceVariant
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (isMe) Arrangement.End else Arrangement.Start,
    ) {
        Surface(
            color = container,
            shape = RoundedCornerShape(12.dp),
            modifier = Modifier.widthIn(max = 320.dp),
        ) {
            Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(
                    text = msg.message,
                    style = MaterialTheme.typography.bodyMedium,
                    color = onContainer,
                )
                msg.createdAt?.let {
                    Text(
                        text = formatDateTime(it),
                        style = MaterialTheme.typography.labelSmall,
                        color = onContainer,
                    )
                }
            }
        }
    }
}
