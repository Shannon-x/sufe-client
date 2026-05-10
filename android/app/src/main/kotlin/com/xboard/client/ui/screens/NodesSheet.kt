package com.xboard.client.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.xboard.client.R
import com.xboard.client.core.ProxyGroup
import com.xboard.client.ui.components.EmptyState
import com.xboard.client.vm.AppViewModel
import kotlinx.coroutines.launch

/**
 * Bottom sheet listing proxy groups + their nodes. Tap a node selects
 * it via [AppViewModel.selectProxy]; "测延迟" triggers
 * [AppViewModel.latencyTest] for every node in the group and stores
 * the result in a sheet-local map (lost on dismiss — re-test on
 * reopen, mirrors the desktop flow).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NodesSheet(viewModel: AppViewModel, onDismiss: () -> Unit) {
    val ui by viewModel.connection.collectAsState()
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val latencies = remember { mutableStateMapOf<String, UInt?>() }
    val scope = rememberCoroutineScope()

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp)
                .padding(bottom = 16.dp),
        ) {
            Text(
                text = stringResource(R.string.connect_button_nodes),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(vertical = 12.dp),
            )

            when {
                ui.proxies.isEmpty() -> {
                    Box(modifier = Modifier.fillMaxWidth().height(160.dp)) {
                        EmptyState(
                            message = stringResource(R.string.connect_nodes_empty),
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                }
                else -> {
                    LazyColumn(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                        items(items = ui.proxies, key = { it.name }) { group ->
                            GroupCard(
                                group = group,
                                latencies = latencies,
                                onPickNode = { node ->
                                    viewModel.selectProxy(group.name, node)
                                },
                                onTestAll = {
                                    scope.launch {
                                        for (node in group.all) {
                                            val ms = viewModel.latencyTest(node)
                                            latencies[node] = ms
                                        }
                                    }
                                },
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun GroupCard(
    group: ProxyGroup,
    latencies: Map<String, UInt?>,
    onPickNode: (String) -> Unit,
    onTestAll: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = group.name,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = group.kind,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            AssistChip(
                onClick = onTestAll,
                label = { Text(stringResource(R.string.connect_nodes_test_all)) },
                leadingIcon = {
                    Icon(
                        imageVector = Icons.Default.Check,
                        contentDescription = null,
                        modifier = Modifier.size(AssistChipDefaults.IconSize),
                    )
                },
            )
        }
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            for (node in group.all) {
                NodeRow(
                    name = node,
                    selected = node == group.now,
                    latencyMs = latencies[node],
                    onClick = { onPickNode(node) },
                )
            }
        }
    }
}

@Composable
private fun NodeRow(
    name: String,
    selected: Boolean,
    latencyMs: UInt?,
    onClick: () -> Unit,
) {
    val container =
        if (selected) MaterialTheme.colorScheme.primaryContainer
        else MaterialTheme.colorScheme.surfaceVariant
    val onContainer =
        if (selected) MaterialTheme.colorScheme.onPrimaryContainer
        else MaterialTheme.colorScheme.onSurfaceVariant

    Surface(
        shape = MaterialTheme.shapes.medium,
        color = container,
        tonalElevation = if (selected) 2.dp else 0.dp,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onClick)
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.weight(1f),
            ) {
                if (selected) {
                    Icon(
                        imageVector = Icons.Default.Check,
                        contentDescription = null,
                        tint = onContainer,
                    )
                }
                Text(
                    text = name,
                    style = MaterialTheme.typography.bodyMedium,
                    color = onContainer,
                )
            }
            LatencyBadge(latencyMs = latencyMs)
        }
    }
}

@Composable
private fun LatencyBadge(latencyMs: UInt?) {
    if (latencyMs == null) return
    val ms = latencyMs.toLong()
    val (label, tint) = when {
        ms == 0L -> stringResource(R.string.connect_nodes_timeout) to MaterialTheme.colorScheme.error
        ms < 200L -> "${ms}ms" to MaterialTheme.colorScheme.tertiary
        ms < 500L -> "${ms}ms" to MaterialTheme.colorScheme.secondary
        else -> "${ms}ms" to MaterialTheme.colorScheme.error
    }
    Text(
        text = label,
        style = MaterialTheme.typography.labelMedium,
        color = tint,
        fontWeight = FontWeight.SemiBold,
    )
}
