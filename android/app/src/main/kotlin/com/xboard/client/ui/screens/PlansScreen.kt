package com.xboard.client.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledTonalButton
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
import com.xboard.client.core.Plan
import com.xboard.client.ui.components.EmptyState
import com.xboard.client.ui.components.LoadingState
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.ui.components.TopProgressStrip
import com.xboard.client.util.formatYuan
import com.xboard.client.vm.AppViewModel

@Composable
fun PlansScreen(viewModel: AppViewModel, onBack: () -> Unit) {
    val state by viewModel.plans.collectAsState()
    val home by viewModel.home.collectAsState()
    var purchaseTarget by remember { mutableStateOf<Plan?>(null) }

    LaunchedEffect(Unit) { viewModel.refreshPlans() }

    ScreenScaffold(
        title = stringResource(R.string.plans_title),
        onBack = onBack,
        actions = {
            IconButton(onClick = { viewModel.refreshPlans() }) {
                Icon(Icons.Default.Refresh, contentDescription = stringResource(R.string.plans_refresh))
            }
        },
    ) { padded ->
        Column(modifier = padded) {
            TopProgressStrip(visible = state.refreshing)
            val plans = state.plans
            when {
                plans == null -> LoadingState(modifier = Modifier.fillMaxSize())
                plans.isEmpty() -> EmptyState(
                    message = stringResource(R.string.plans_empty),
                    modifier = Modifier.fillMaxSize(),
                )
                else -> ScrollableColumn(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    val currentPlanId = home.user?.planId
                    for (plan in plans) {
                        PlanCard(
                            plan = plan,
                            isCurrent = currentPlanId == plan.id,
                            onBuy = { purchaseTarget = plan },
                        )
                    }
                }
            }
        }
    }

    purchaseTarget?.let { plan ->
        PurchaseSheet(
            viewModel = viewModel,
            plan = plan,
            onDismiss = { purchaseTarget = null },
        )
    }
}

@Composable
private fun PlanCard(plan: Plan, isCurrent: Boolean, onBuy: () -> Unit) {
    val periods = collectPeriods(plan)
    Card(modifier = Modifier.fillMaxWidth(), colors = CardDefaults.elevatedCardColors()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    text = plan.name,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    if (isCurrent) {
                        AssistChip(
                            onClick = {},
                            label = { Text(stringResource(R.string.plans_tag_current)) },
                            colors = AssistChipDefaults.assistChipColors(
                                containerColor = MaterialTheme.colorScheme.primaryContainer,
                                labelColor = MaterialTheme.colorScheme.onPrimaryContainer,
                            ),
                        )
                    }
                    if (!plan.sell) {
                        AssistChip(
                            onClick = {},
                            label = { Text(stringResource(R.string.plans_tag_not_selling)) },
                        )
                    }
                    if (!plan.renew && plan.sell) {
                        AssistChip(
                            onClick = {},
                            label = { Text(stringResource(R.string.plans_tag_no_renew)) },
                        )
                    }
                }
            }
            Text(
                text = stringResource(R.string.plans_transfer_enable, plan.transferEnable.toString()),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (plan.content.isNotBlank()) {
                Text(
                    text = plan.content,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (periods.isEmpty()) {
                Text(
                    text = stringResource(R.string.plans_no_prices),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.error,
                )
            } else {
                Box(modifier = Modifier.fillMaxWidth()) {
                    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        for (p in periods) {
                            Text(
                                text = "${stringResource(p.labelRes)}  ¥${formatYuan(p.priceCents)}",
                                style = MaterialTheme.typography.labelLarge,
                            )
                        }
                    }
                }
                FilledTonalButton(
                    onClick = onBuy,
                    enabled = plan.sell,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(stringResource(R.string.plans_buy))
                }
            }
        }
    }
}

internal data class PeriodOption(
    val key: String,
    val labelRes: Int,
    val priceCents: Long,
)

internal fun collectPeriods(plan: Plan): List<PeriodOption> = buildList {
    plan.monthPrice?.let { add(PeriodOption("month_price", R.string.plans_period_month, it)) }
    plan.quarterPrice?.let { add(PeriodOption("quarter_price", R.string.plans_period_quarter, it)) }
    plan.halfYearPrice?.let { add(PeriodOption("half_year_price", R.string.plans_period_half_year, it)) }
    plan.yearPrice?.let { add(PeriodOption("year_price", R.string.plans_period_year, it)) }
    plan.twoYearPrice?.let { add(PeriodOption("two_year_price", R.string.plans_period_two_year, it)) }
    plan.threeYearPrice?.let { add(PeriodOption("three_year_price", R.string.plans_period_three_year, it)) }
    plan.onetimePrice?.let { add(PeriodOption("onetime_price", R.string.plans_period_onetime, it)) }
    plan.resetPrice?.let { add(PeriodOption("reset_price", R.string.plans_reset_price, it)) }
}
