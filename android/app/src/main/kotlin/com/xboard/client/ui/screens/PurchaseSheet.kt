package com.xboard.client.ui.screens

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.xboard.client.R
import com.xboard.client.core.PaymentMethod
import com.xboard.client.core.Plan
import com.xboard.client.core.SaveOrderArgs
import com.xboard.client.util.formatYuan
import com.xboard.client.vm.AppViewModel
import kotlinx.coroutines.launch
import org.json.JSONObject
import org.json.JSONTokener

private sealed interface PurchasePhase {
    data object Form : PurchasePhase
    data object Submitting : PurchasePhase
    data class AwaitingGateway(val tradeNo: String, val redirect: String?) : PurchasePhase
    data class BalancePaid(val tradeNo: String) : PurchasePhase
    data class Status(val tradeNo: String, val status: Int?) : PurchasePhase
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PurchaseSheet(
    viewModel: AppViewModel,
    plan: Plan,
    onDismiss: () -> Unit,
) {
    val ctx = LocalContext.current
    val plans by viewModel.plans.collectAsState()
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()

    val periods = remember(plan) { collectPeriods(plan) }
    var selectedPeriod by remember(plan) { mutableStateOf(periods.firstOrNull()) }
    var coupon by remember(plan) { mutableStateOf("") }
    var selectedPaymentId by remember { mutableStateOf<Long?>(null) }
    var localError by remember { mutableStateOf<String?>(null) }
    var phase by remember { mutableStateOf<PurchasePhase>(PurchasePhase.Form) }

    LaunchedEffect(Unit) {
        if (plans.paymentMethods == null) viewModel.refreshPlans()
    }

    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheetState) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp)
                .padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                text = stringResource(R.string.purchase_title),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(top = 8.dp),
            )
            Text(
                text = "${stringResource(R.string.purchase_plan)}: ${plan.name}",
                style = MaterialTheme.typography.bodyMedium,
            )

            when (val p = phase) {
                PurchasePhase.Form, PurchasePhase.Submitting -> FormSection(
                    periods = periods,
                    selectedPeriod = selectedPeriod,
                    onPickPeriod = { selectedPeriod = it; localError = null },
                    coupon = coupon,
                    onCouponChange = { coupon = it; localError = null },
                    paymentMethods = plans.paymentMethods,
                    selectedPaymentId = selectedPaymentId,
                    onPickPayment = { selectedPaymentId = it; localError = null },
                    submitting = phase is PurchasePhase.Submitting,
                    localError = localError,
                    onSubmit = submit@{
                        val period = selectedPeriod
                        val pay = selectedPaymentId
                        if (period == null) {
                            localError = ctx.getString(R.string.plans_no_prices)
                            return@submit
                        }
                        if (pay == null) {
                            localError = ctx.getString(R.string.purchase_error_pick_payment)
                            return@submit
                        }
                        phase = PurchasePhase.Submitting
                        scope.launch {
                            val tradeNo = viewModel.saveOrder(
                                SaveOrderArgs(
                                    planId = plan.id,
                                    period = period.key,
                                    couponCode = coupon.trim().takeIf { it.isNotBlank() },
                                ),
                            )
                            if (tradeNo.isNullOrBlank()) {
                                localError = ctx.getString(R.string.purchase_error_no_trade_no)
                                phase = PurchasePhase.Form
                                return@launch
                            }
                            val resp = viewModel.checkoutOrder(tradeNo, pay)
                            phase = if (resp == null) {
                                PurchasePhase.BalancePaid(tradeNo)
                            } else when (resp.kind) {
                                1, -2 -> PurchasePhase.AwaitingGateway(tradeNo, extractUrl(resp.dataJson))
                                0 -> PurchasePhase.AwaitingGateway(tradeNo, extractUrl(resp.dataJson))
                                else -> PurchasePhase.BalancePaid(tradeNo)
                            }
                        }
                    },
                    onCancel = onDismiss,
                )

                is PurchasePhase.AwaitingGateway -> AwaitingGatewaySection(
                    redirect = p.redirect,
                    onOpenBrowser = { url ->
                        runCatching {
                            ctx.startActivity(
                                Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(
                                    Intent.FLAG_ACTIVITY_NEW_TASK,
                                ),
                            )
                        }.onFailure {
                            viewModel.emitClipboardCopy(
                                text = url,
                                toastMessage = ctx.getString(R.string.purchase_qr_copied),
                            )
                        }
                    },
                    onCopyLink = { url ->
                        viewModel.emitClipboardCopy(
                            text = url,
                            toastMessage = ctx.getString(R.string.purchase_qr_copied),
                        )
                    },
                    onRefreshStatus = {
                        scope.launch {
                            val s = viewModel.checkOrder(p.tradeNo)
                            phase = PurchasePhase.Status(p.tradeNo, s)
                        }
                    },
                    onCancel = {
                        scope.launch {
                            if (viewModel.cancelOrder(p.tradeNo)) {
                                viewModel.refreshOrders()
                                onDismiss()
                            }
                        }
                    },
                    onClose = onDismiss,
                )

                is PurchasePhase.BalancePaid -> SimpleStatusSection(
                    title = stringResource(R.string.purchase_balance_paid),
                    onRefreshStatus = {
                        scope.launch {
                            val s = viewModel.checkOrder(p.tradeNo)
                            phase = PurchasePhase.Status(p.tradeNo, s)
                        }
                    },
                    onClose = {
                        viewModel.refreshHome()
                        viewModel.refreshOrders()
                        onDismiss()
                    },
                )

                is PurchasePhase.Status -> StatusSection(
                    statusInt = p.status,
                    onRefreshStatus = {
                        scope.launch {
                            val s = viewModel.checkOrder(p.tradeNo)
                            phase = PurchasePhase.Status(p.tradeNo, s)
                        }
                    },
                    onCancel = {
                        scope.launch {
                            if (viewModel.cancelOrder(p.tradeNo)) {
                                viewModel.refreshOrders()
                                onDismiss()
                            }
                        }
                    },
                    onClose = {
                        viewModel.refreshHome()
                        viewModel.refreshOrders()
                        onDismiss()
                    },
                )
            }
        }
    }
}

@Composable
private fun FormSection(
    periods: List<PeriodOption>,
    selectedPeriod: PeriodOption?,
    onPickPeriod: (PeriodOption) -> Unit,
    coupon: String,
    onCouponChange: (String) -> Unit,
    paymentMethods: List<PaymentMethod>?,
    selectedPaymentId: Long?,
    onPickPayment: (Long) -> Unit,
    submitting: Boolean,
    localError: String?,
    onSubmit: () -> Unit,
    onCancel: () -> Unit,
) {
    Text(stringResource(R.string.purchase_period), style = MaterialTheme.typography.labelLarge)
    Column(modifier = Modifier.selectableGroup()) {
        for (option in periods) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .selectable(
                        selected = selectedPeriod == option,
                        onClick = { onPickPeriod(option) },
                    )
                    .padding(vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = selectedPeriod == option, onClick = null)
                Spacer(Modifier.width(8.dp))
                Text(
                    text = "${stringResource(option.labelRes)}  ¥${formatYuan(option.priceCents)}",
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }

    OutlinedTextField(
        value = coupon,
        onValueChange = onCouponChange,
        label = { Text(stringResource(R.string.purchase_coupon_label)) },
        placeholder = { Text(stringResource(R.string.purchase_coupon_placeholder)) },
        singleLine = true,
        enabled = !submitting,
        modifier = Modifier.fillMaxWidth(),
    )

    Text(stringResource(R.string.purchase_payment_method), style = MaterialTheme.typography.labelLarge)
    when {
        paymentMethods == null -> Text(
            stringResource(R.string.purchase_payment_method_loading),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        paymentMethods.isEmpty() -> Text(
            stringResource(R.string.purchase_payment_method_empty),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.error,
        )
        else -> Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            for (pm in paymentMethods) {
                PaymentMethodTile(
                    pm = pm,
                    selected = selectedPaymentId == pm.id,
                    onClick = { onPickPayment(pm.id) },
                )
            }
        }
    }

    if (localError != null) {
        Text(
            text = localError,
            color = MaterialTheme.colorScheme.error,
            style = MaterialTheme.typography.bodySmall,
        )
    }

    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        OutlinedButton(onClick = onCancel, enabled = !submitting, modifier = Modifier.weight(1f)) {
            Text(stringResource(R.string.purchase_cancel))
        }
        Button(
            onClick = onSubmit,
            enabled = !submitting,
            modifier = Modifier.weight(1f).height(48.dp),
        ) {
            if (submitting) {
                CircularProgressIndicator(
                    modifier = Modifier.height(20.dp),
                    strokeWidth = 2.dp,
                    color = MaterialTheme.colorScheme.onPrimary,
                )
            } else {
                Text(stringResource(R.string.purchase_submit))
            }
        }
    }
}

@Composable
private fun PaymentMethodTile(pm: PaymentMethod, selected: Boolean, onClick: () -> Unit) {
    val container =
        if (selected) MaterialTheme.colorScheme.primaryContainer
        else MaterialTheme.colorScheme.surfaceVariant
    val onContainer =
        if (selected) MaterialTheme.colorScheme.onPrimaryContainer
        else MaterialTheme.colorScheme.onSurfaceVariant

    val feeFixed = pm.handlingFeeFixed?.let {
        stringResource(R.string.purchase_fee_fixed, formatYuan(it))
    }
    val feePercent = pm.handlingFeePercent?.let {
        stringResource(R.string.purchase_fee_percent, "%.2f".format(it))
    }
    val fee = listOfNotNull(feeFixed, feePercent).joinToString("  ")

    Surface(
        shape = RoundedCornerShape(12.dp),
        color = container,
        tonalElevation = if (selected) 2.dp else 0.dp,
        modifier = Modifier.fillMaxWidth(),
        onClick = onClick,
    ) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                text = pm.name,
                style = MaterialTheme.typography.bodyMedium,
                color = onContainer,
                fontWeight = FontWeight.SemiBold,
            )
            if (fee.isNotEmpty()) {
                Text(text = fee, style = MaterialTheme.typography.labelSmall, color = onContainer)
            }
        }
    }
}

@Composable
private fun AwaitingGatewaySection(
    redirect: String?,
    onOpenBrowser: (String) -> Unit,
    onCopyLink: (String) -> Unit,
    onRefreshStatus: () -> Unit,
    onCancel: () -> Unit,
    onClose: () -> Unit,
) {
    Text(
        text = stringResource(R.string.purchase_pending_title),
        style = MaterialTheme.typography.titleSmall,
        fontWeight = FontWeight.SemiBold,
    )
    if (!redirect.isNullOrBlank()) {
        Text(stringResource(R.string.purchase_qr_show), style = MaterialTheme.typography.bodySmall)
        Surface(
            shape = RoundedCornerShape(8.dp),
            color = MaterialTheme.colorScheme.surfaceVariant,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(
                text = redirect,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(12.dp),
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = { onCopyLink(redirect) }, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.purchase_qr_copy))
            }
            Button(onClick = { onOpenBrowser(redirect) }, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.purchase_redirect_opened))
            }
        }
    }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        OutlinedButton(onClick = onCancel, modifier = Modifier.weight(1f)) {
            Text(stringResource(R.string.purchase_cancel_order))
        }
        OutlinedButton(onClick = onRefreshStatus, modifier = Modifier.weight(1f)) {
            Text(stringResource(R.string.purchase_status_refresh))
        }
        TextButton(onClick = onClose, modifier = Modifier.weight(1f)) {
            Text(stringResource(R.string.purchase_close))
        }
    }
}

@Composable
private fun SimpleStatusSection(
    title: String,
    onRefreshStatus: () -> Unit,
    onClose: () -> Unit,
) {
    Text(text = title, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedButton(onClick = onRefreshStatus, modifier = Modifier.weight(1f)) {
            Text(stringResource(R.string.purchase_status_refresh))
        }
        Button(onClick = onClose, modifier = Modifier.weight(1f)) {
            Text(stringResource(R.string.purchase_close))
        }
    }
}

@Composable
private fun StatusSection(
    statusInt: Int?,
    onRefreshStatus: () -> Unit,
    onCancel: () -> Unit,
    onClose: () -> Unit,
) {
    val statusText = when (statusInt) {
        0 -> stringResource(R.string.purchase_status_pending)
        1 -> stringResource(R.string.purchase_status_activating)
        2 -> stringResource(R.string.purchase_status_cancelled)
        3 -> stringResource(R.string.purchase_status_completed)
        else -> stringResource(R.string.purchase_status_unknown, statusInt ?: -1)
    }
    val finished = statusInt == 2 || statusInt == 3
    Text(text = statusText, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        if (!finished) {
            OutlinedButton(onClick = onCancel, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.purchase_cancel_order))
            }
            OutlinedButton(onClick = onRefreshStatus, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.purchase_status_refresh))
            }
        }
        Button(onClick = onClose, modifier = Modifier.weight(1f)) {
            Text(stringResource(R.string.purchase_close))
        }
    }
}

private fun extractUrl(dataJson: String?): String? {
    if (dataJson.isNullOrBlank()) return null
    val parsed = runCatching { JSONTokener(dataJson).nextValue() }.getOrNull()
    return when (parsed) {
        is String -> parsed
        is JSONObject -> parsed.optString("url").ifBlank {
            parsed.optString("redirect").ifBlank {
                parsed.optString("qr_code").ifBlank { dataJson }
            }
        }
        else -> dataJson
    }
}

