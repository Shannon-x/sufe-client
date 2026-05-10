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
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.PowerSettingsNew
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.xboard.client.R
import com.xboard.client.core.ConnectStage
import com.xboard.client.core.ConnectionState
import com.xboard.client.core.TunnelMode
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.util.formatBytes
import com.xboard.client.vm.AppViewModel
import com.xboard.client.vpn.VpnBinder

@Composable
fun ConnectScreen(
    viewModel: AppViewModel,
    vpnBinder: VpnBinder,
    onBack: () -> Unit,
) {
    val ui by viewModel.connection.collectAsState()
    var nodesOpen by remember { mutableStateOf(false) }

    ScreenScaffold(title = stringResource(R.string.connect_title), onBack = onBack) { padded ->
        ScrollableColumn(
            modifier = padded,
            verticalArrangement = Arrangement.spacedBy(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            ConnectionToggle(
                state = ui.state,
                onConnect = { viewModel.connect(vpnBinder) },
                onDisconnect = { viewModel.disconnect() },
            )

            StatusCaption(state = ui.state, errorMessage = ui.errorMessage)

            ModeSwitch(
                current = ui.mode,
                onPick = { viewModel.setMode(it) },
            )

            CurrentNodeRow(
                nodeName = ui.selectedNode,
                onOpenNodes = {
                    viewModel.refreshProxies()
                    nodesOpen = true
                },
            )

            if (ui.state is ConnectionState.Connected) {
                TrafficCard(
                    upPerSec = ui.traffic?.up?.toLong() ?: 0L,
                    downPerSec = ui.traffic?.down?.toLong() ?: 0L,
                    upTotal = ui.traffic?.upTotal?.toLong() ?: 0L,
                    downTotal = ui.traffic?.downTotal?.toLong() ?: 0L,
                )
            }

            Spacer(Modifier.height(8.dp))
            TextButton(onClick = { /* logs view reserved */ }) {
                Text(stringResource(R.string.connect_view_logs))
            }
        }
    }

    if (nodesOpen) {
        NodesSheet(
            viewModel = viewModel,
            onDismiss = { nodesOpen = false },
        )
    }
}

@Composable
private fun ConnectionToggle(
    state: ConnectionState,
    onConnect: () -> Unit,
    onDisconnect: () -> Unit,
) {
    val connecting = state is ConnectionState.Connecting
    val connected = state is ConnectionState.Connected
    val tint = when {
        connected -> MaterialTheme.colorScheme.primary
        connecting -> MaterialTheme.colorScheme.tertiary
        else -> MaterialTheme.colorScheme.surfaceVariant
    }
    val onTint = when {
        connected -> MaterialTheme.colorScheme.onPrimary
        connecting -> MaterialTheme.colorScheme.onTertiary
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }

    Surface(
        shape = CircleShape,
        color = tint,
        modifier = Modifier.size(168.dp),
        onClick = {
            if (connecting) return@Surface
            if (connected) onDisconnect() else onConnect()
        },
    ) {
        Box(contentAlignment = Alignment.Center) {
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                if (connecting) {
                    CircularProgressIndicator(
                        color = onTint,
                        strokeWidth = 4.dp,
                        modifier = Modifier.size(56.dp),
                    )
                } else {
                    Icon(
                        imageVector = Icons.Default.PowerSettingsNew,
                        contentDescription = null,
                        tint = onTint,
                        modifier = Modifier.size(56.dp),
                    )
                }
                Spacer(Modifier.height(8.dp))
                Text(
                    text = stringResource(
                        if (connected) R.string.connect_button_connected
                        else R.string.connect_button_disconnected,
                    ),
                    color = onTint,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
            }
        }
    }
}

@Composable
private fun StatusCaption(state: ConnectionState, errorMessage: String?) {
    val text = when (state) {
        is ConnectionState.Disconnected ->
            errorMessage?.let { stringResource(R.string.connect_status_error, it) }
                ?: stringResource(R.string.connect_status_disconnected)
        is ConnectionState.Connecting -> when (state.stage) {
            ConnectStage.FETCHING -> stringResource(R.string.connect_status_fetching)
            ConnectStage.WRITING -> stringResource(R.string.connect_status_writing)
            ConnectStage.ELEVATING -> stringResource(R.string.connect_status_elevating)
            ConnectStage.SPAWNING -> stringResource(R.string.connect_status_spawning)
            ConnectStage.APPLYING_ROUTE -> stringResource(R.string.connect_status_applying_route)
            ConnectStage.FALLBACK_PROXY -> stringResource(R.string.connect_status_elevating)
        }
        is ConnectionState.Connected -> stringResource(R.string.connect_status_connected)
        is ConnectionState.Failed -> stringResource(R.string.connect_status_error, state.message)
    }
    Text(
        text = text,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun ModeSwitch(current: TunnelMode, onPick: (TunnelMode) -> Unit) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = stringResource(R.string.connect_mode_label),
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilterChip(
                selected = current == TunnelMode.TUN,
                onClick = { onPick(TunnelMode.TUN) },
                label = { Text(stringResource(R.string.connect_mode_tun)) },
                leadingIcon = if (current == TunnelMode.TUN) {
                    {
                        Icon(
                            imageVector = Icons.Default.Bolt,
                            contentDescription = null,
                            modifier = Modifier.size(FilterChipDefaults.IconSize),
                        )
                    }
                } else null,
            )
            FilterChip(
                selected = current == TunnelMode.SYSTEM_PROXY,
                onClick = { onPick(TunnelMode.SYSTEM_PROXY) },
                label = { Text(stringResource(R.string.connect_mode_system_proxy)) },
            )
        }
    }
}

@Composable
private fun CurrentNodeRow(nodeName: String?, onOpenNodes: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    stringResource(R.string.connect_current_node),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    text = nodeName ?: "—",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
            }
            OutlinedButton(onClick = onOpenNodes) {
                Text(stringResource(R.string.connect_button_nodes))
            }
        }
    }
}

@Composable
private fun TrafficCard(
    upPerSec: Long,
    downPerSec: Long,
    upTotal: Long,
    downTotal: Long,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.elevatedCardColors(),
    ) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(
                stringResource(R.string.connect_live_traffic),
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.SemiBold,
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                TrafficStat(
                    label = "↑ ${formatBytes(upPerSec)}/s",
                    sub = "Σ ${formatBytes(upTotal)}",
                )
                TrafficStat(
                    label = "↓ ${formatBytes(downPerSec)}/s",
                    sub = "Σ ${formatBytes(downTotal)}",
                )
            }
        }
    }
}

@Composable
private fun TrafficStat(label: String, sub: String) {
    Column {
        Text(text = label, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        Text(
            text = sub,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
