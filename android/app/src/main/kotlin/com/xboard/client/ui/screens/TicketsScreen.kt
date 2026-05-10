package com.xboard.client.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
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
import com.xboard.client.core.Ticket
import com.xboard.client.ui.components.EmptyState
import com.xboard.client.ui.components.LoadingState
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.ui.components.TopProgressStrip
import com.xboard.client.util.formatDateTime
import com.xboard.client.vm.AppViewModel

@Composable
fun TicketsScreen(
    viewModel: AppViewModel,
    onBack: () -> Unit,
    onOpen: (Long) -> Unit,
) {
    val state by viewModel.tickets.collectAsState()
    var composerOpen by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) { viewModel.refreshTickets() }

    ScreenScaffold(
        title = stringResource(R.string.tickets_title),
        onBack = onBack,
        actions = {
            IconButton(onClick = { viewModel.refreshTickets() }) {
                Icon(Icons.Default.Refresh, contentDescription = stringResource(R.string.tickets_refresh))
            }
        },
    ) { padded ->
        Box(modifier = padded) {
            Column(modifier = Modifier.fillMaxSize()) {
                TopProgressStrip(visible = state.refreshing)
                val tickets = state.tickets
                when {
                    tickets == null -> LoadingState(modifier = Modifier.fillMaxSize())
                    tickets.isEmpty() -> EmptyState(
                        message = stringResource(R.string.tickets_empty),
                        modifier = Modifier.fillMaxSize(),
                    )
                    else -> ScrollableColumn(
                        modifier = Modifier.fillMaxWidth(),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        for (t in tickets) {
                            TicketRow(ticket = t, onClick = { onOpen(t.id) })
                        }
                    }
                }
            }

            ExtendedFloatingActionButton(
                onClick = { composerOpen = true },
                icon = { Icon(Icons.Default.Add, contentDescription = null) },
                text = { Text(stringResource(R.string.tickets_composer_new)) },
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(16.dp),
            )
        }
    }

    if (composerOpen) {
        TicketComposerSheet(
            viewModel = viewModel,
            onDismiss = { composerOpen = false },
        )
    }
}

@Composable
private fun TicketRow(ticket: Ticket, onClick: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth().clickable(onClick = onClick)) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = ticket.subject,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.weight(1f),
                )
                LevelChip(level = ticket.level)
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                StatusChip(status = ticket.status)
                Text(
                    text = ticket.updatedAt?.let { formatDateTime(it) } ?: "",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
internal fun LevelChip(level: Int) {
    val (textRes, fg, bg) = when (level) {
        0 -> Triple(
            R.string.tickets_level_low,
            MaterialTheme.colorScheme.onSurfaceVariant,
            MaterialTheme.colorScheme.surfaceVariant,
        )
        1 -> Triple(
            R.string.tickets_level_normal,
            MaterialTheme.colorScheme.onSecondaryContainer,
            MaterialTheme.colorScheme.secondaryContainer,
        )
        else -> Triple(
            R.string.tickets_level_high,
            MaterialTheme.colorScheme.onErrorContainer,
            MaterialTheme.colorScheme.errorContainer,
        )
    }
    AssistChip(
        onClick = {},
        label = { Text(stringResource(textRes)) },
        colors = AssistChipDefaults.assistChipColors(containerColor = bg, labelColor = fg),
    )
}

@Composable
internal fun StatusChip(status: Int) {
    val (textRes, fg, bg) = if (status == 0) {
        Triple(
            R.string.tickets_status_open,
            MaterialTheme.colorScheme.onTertiaryContainer,
            MaterialTheme.colorScheme.tertiaryContainer,
        )
    } else {
        Triple(
            R.string.tickets_status_closed,
            MaterialTheme.colorScheme.onSurfaceVariant,
            MaterialTheme.colorScheme.surfaceVariant,
        )
    }
    AssistChip(
        onClick = {},
        label = { Text(stringResource(textRes)) },
        colors = AssistChipDefaults.assistChipColors(containerColor = bg, labelColor = fg),
    )
}
