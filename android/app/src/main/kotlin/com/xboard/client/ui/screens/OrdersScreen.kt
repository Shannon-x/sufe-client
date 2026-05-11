package com.xboard.client.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.xboard.client.R
import com.xboard.client.core.Order
import com.xboard.client.ui.components.EmptyState
import com.xboard.client.ui.components.LoadingState
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.ui.components.TopProgressStrip
import com.xboard.client.util.formatDateTime
import com.xboard.client.util.formatYuan
import com.xboard.client.vm.AppViewModel
import kotlinx.coroutines.launch

@Composable
fun OrdersScreen(viewModel: AppViewModel, onBack: () -> Unit) {
    val state by viewModel.orders.collectAsState()
    val scope = rememberCoroutineScope()

    LaunchedEffect(Unit) { viewModel.refreshOrders() }

    ScreenScaffold(
        title = stringResource(R.string.orders_title),
        onBack = onBack,
        actions = {
            IconButton(onClick = { viewModel.refreshOrders() }) {
                Icon(Icons.Default.Refresh, contentDescription = stringResource(R.string.orders_refresh))
            }
        },
    ) { padded ->
        Column(modifier = padded) {
            TopProgressStrip(visible = state.refreshing)
            val list = state.orders
            when {
                list == null -> LoadingState(modifier = Modifier.fillMaxSize())
                list.isEmpty() -> EmptyState(
                    message = stringResource(R.string.orders_empty),
                    modifier = Modifier.fillMaxSize(),
                )
                else -> ScrollableColumn(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    for (order in list) {
                        OrderCard(
                            order = order,
                            onCancel = {
                                scope.launch {
                                    if (viewModel.cancelOrder(order.tradeNo)) {
                                        viewModel.refreshOrders()
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

@Composable
private fun OrderCard(order: Order, onCancel: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = order.tradeNo,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                OrderStatusChip(status = order.status)
            }

            order.kind?.let { OrderMeta(label = stringResource(R.string.orders_col_kind), value = kindLabel(it)) }
            order.period?.let {
                OrderMeta(label = stringResource(R.string.orders_col_period), value = periodLabel(it))
            }
            OrderMeta(
                label = stringResource(R.string.orders_col_amount),
                value = "¥${formatYuan(order.totalAmount)}",
            )
            order.createdAt?.let {
                OrderMeta(
                    label = stringResource(R.string.orders_col_created_at),
                    value = formatDateTime(it),
                )
            }

            if (order.status == 0) {
                Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                    OutlinedButton(onClick = onCancel) {
                        Text(stringResource(R.string.orders_action_cancel))
                    }
                }
            }
        }
    }
}

@Composable
private fun OrderMeta(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(text = value, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun OrderStatusChip(status: Int) {
    val (textRes, fg, bg) = when (status) {
        0 -> Triple(
            R.string.orders_status_pending,
            MaterialTheme.colorScheme.onTertiaryContainer,
            MaterialTheme.colorScheme.tertiaryContainer,
        )
        1 -> Triple(
            R.string.orders_status_activating,
            MaterialTheme.colorScheme.onSecondaryContainer,
            MaterialTheme.colorScheme.secondaryContainer,
        )
        2 -> Triple(
            R.string.orders_status_cancelled,
            MaterialTheme.colorScheme.onErrorContainer,
            MaterialTheme.colorScheme.errorContainer,
        )
        3 -> Triple(
            R.string.orders_status_completed,
            MaterialTheme.colorScheme.onPrimaryContainer,
            MaterialTheme.colorScheme.primaryContainer,
        )
        4 -> Triple(
            R.string.orders_status_discounted,
            MaterialTheme.colorScheme.onPrimaryContainer,
            MaterialTheme.colorScheme.primaryContainer,
        )
        else -> Triple(
            R.string.orders_status_pending,
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

@Composable
private fun kindLabel(kind: Int): String = stringResource(
    when (kind) {
        1 -> R.string.orders_kind_new
        2 -> R.string.orders_kind_renew
        3 -> R.string.orders_kind_upgrade
        4 -> R.string.orders_kind_reset
        else -> R.string.orders_kind_new
    },
)

@Composable
private fun periodLabel(period: String): String = when (period) {
    "month_price" -> stringResource(R.string.plans_period_month)
    "quarter_price" -> stringResource(R.string.plans_period_quarter)
    "half_year_price" -> stringResource(R.string.plans_period_half_year)
    "year_price" -> stringResource(R.string.plans_period_year)
    "two_year_price" -> stringResource(R.string.plans_period_two_year)
    "three_year_price" -> stringResource(R.string.plans_period_three_year)
    "onetime_price" -> stringResource(R.string.plans_period_onetime)
    "reset_price" -> stringResource(R.string.plans_reset_price)
    else -> period
}
