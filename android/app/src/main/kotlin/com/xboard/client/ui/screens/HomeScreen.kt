package com.xboard.client.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.xboard.client.R
import com.xboard.client.core.SubscribeInfo
import com.xboard.client.core.UserInfo
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.ui.components.TopProgressStrip
import com.xboard.client.util.formatBytes
import com.xboard.client.util.formatDate
import com.xboard.client.util.formatYuan
import com.xboard.client.vm.AppViewModel
import com.xboard.client.vm.AuthState

@Composable
fun HomeScreen(
    viewModel: AppViewModel,
    onOpenConnect: () -> Unit,
    onOpenPlans: () -> Unit,
    onOpenOrders: () -> Unit,
    onOpenTickets: () -> Unit,
    onOpenNotices: () -> Unit,
) {
    val home by viewModel.home.collectAsState()
    val auth by viewModel.authState.collectAsState()
    val ctx = LocalContext.current
    val email = (auth as? AuthState.Authenticated)?.summary?.email ?: home.user?.email ?: ""

    ScreenScaffold(
        title = stringResource(R.string.app_name),
        onBack = null,
        actions = {
            IconButton(onClick = { viewModel.refreshHome() }) {
                Icon(Icons.Default.Refresh, contentDescription = stringResource(R.string.home_refresh))
            }
            IconButton(onClick = { viewModel.logout() }) {
                Icon(Icons.AutoMirrored.Filled.Logout, contentDescription = stringResource(R.string.home_logout))
            }
        },
    ) { padded ->
        Column(modifier = padded) {
            TopProgressStrip(visible = home.refreshing)
            ScrollableColumn(
                modifier = Modifier.fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(
                    text = stringResource(R.string.home_welcome, email),
                    style = MaterialTheme.typography.titleMedium,
                )

                ExtendedFloatingActionButton(
                    text = { Text(stringResource(R.string.connect_title)) },
                    icon = {
                        Box(
                            modifier = Modifier
                                .size(10.dp)
                                .padding(end = 4.dp),
                        )
                    },
                    onClick = onOpenConnect,
                    modifier = Modifier.fillMaxWidth(),
                )

                home.user?.let { UserCard(it) }
                home.subscribe?.let { info ->
                    SubscribeCard(
                        sub = info,
                        onCopy = {
                            viewModel.emitClipboardCopy(
                                text = info.subscribeUrl,
                                toastMessage = ctx.getString(R.string.home_copied),
                            )
                        },
                    )
                }
                if (home.user != null && home.subscribe?.planId == null) {
                    Card(modifier = Modifier.fillMaxWidth()) {
                        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text(
                                stringResource(R.string.home_cue_none_title),
                                style = MaterialTheme.typography.titleSmall,
                                fontWeight = FontWeight.SemiBold,
                            )
                            Text(stringResource(R.string.home_cue_none_body))
                            FilledTonalButton(onClick = onOpenPlans) {
                                Text(stringResource(R.string.home_cue_browse))
                            }
                        }
                    }
                }

                MenuGrid(
                    onPlans = onOpenPlans,
                    onOrders = onOpenOrders,
                    onTickets = onOpenTickets,
                    onNotices = onOpenNotices,
                )

                home.notices?.takeIf { it.isNotEmpty() }?.let { list ->
                    Text(
                        text = stringResource(R.string.home_notices),
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.SemiBold,
                    )
                    list.take(3).forEach { notice ->
                        Card(modifier = Modifier.fillMaxWidth()) {
                            Column(modifier = Modifier.padding(12.dp)) {
                                Text(
                                    text = notice.title.ifBlank {
                                        stringResource(R.string.notices_untitled)
                                    },
                                    style = MaterialTheme.typography.titleSmall,
                                )
                                Text(
                                    text = notice.content,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    maxLines = 2,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun UserCard(user: UserInfo) {
    Card(modifier = Modifier.fillMaxWidth(), colors = CardDefaults.elevatedCardColors()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(user.email, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                Stat(label = stringResource(R.string.home_balance), value = "¥${formatYuan(user.balance)}")
                Stat(label = stringResource(R.string.home_commission), value = "¥${formatYuan(user.commissionBalance)}")
            }
        }
    }
}

@Composable
private fun SubscribeCard(sub: SubscribeInfo, onCopy: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(stringResource(R.string.home_subscribe), style = MaterialTheme.typography.titleSmall)
                IconButton(onClick = onCopy) {
                    Icon(Icons.Default.ContentCopy, contentDescription = stringResource(R.string.home_copy))
                }
            }
            Text(
                text = sub.subscribeUrl,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 2,
            )
            val total = sub.transferEnable
            val used = sub.upload + sub.download
            val ratio = if (total == 0UL) 0f else (used.toFloat() / total.toFloat()).coerceIn(0f, 1f)
            LinearProgressIndicator(progress = { ratio }, modifier = Modifier.fillMaxWidth())
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    text = "${stringResource(R.string.home_used)} ${formatBytes(used)}",
                    style = MaterialTheme.typography.labelMedium,
                )
                Text(
                    text = "${stringResource(R.string.home_total)} ${formatBytes(total)}",
                    style = MaterialTheme.typography.labelMedium,
                )
            }
            sub.expiredAt?.let {
                Text(
                    text = "${stringResource(R.string.home_expiry)}: ${formatDate(it)}",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } ?: Text(
                text = "${stringResource(R.string.home_expiry)}: ${stringResource(R.string.home_expiry_never)}",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun Stat(label: String, value: String) {
    Column {
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
    }
}

@Composable
private fun MenuGrid(
    onPlans: () -> Unit,
    onOrders: () -> Unit,
    onTickets: () -> Unit,
    onNotices: () -> Unit,
) {
    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        MenuItem(stringResource(R.string.home_menu_plans), modifier = Modifier.weight(1f), onClick = onPlans)
        MenuItem(stringResource(R.string.home_menu_orders), modifier = Modifier.weight(1f), onClick = onOrders)
    }
    Spacer(Modifier.height(8.dp))
    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        MenuItem(stringResource(R.string.home_menu_tickets), modifier = Modifier.weight(1f), onClick = onTickets)
        MenuItem(stringResource(R.string.home_menu_notices), modifier = Modifier.weight(1f), onClick = onNotices)
    }
}

@Composable
private fun MenuItem(label: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Surface(
        modifier = modifier.height(72.dp),
        onClick = onClick,
        shape = MaterialTheme.shapes.medium,
        tonalElevation = 1.dp,
        color = MaterialTheme.colorScheme.surfaceVariant,
    ) {
        Box(contentAlignment = Alignment.Center) {
            Text(label, style = MaterialTheme.typography.titleSmall)
        }
    }
}
