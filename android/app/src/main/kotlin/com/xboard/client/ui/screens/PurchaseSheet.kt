package com.xboard.client.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xboard.client.core.Plan
import com.xboard.client.core.SaveOrderArgs
import com.xboard.client.util.formatYuan
import com.xboard.client.vm.AppViewModel
import kotlinx.coroutines.launch

/** An uncertain save is never retried automatically or silently cancelled. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PurchaseSheet(viewModel: AppViewModel, plan: Plan, onDismiss: () -> Unit) {
    val state by viewModel.business.state.collectAsState()
    val periods = remember(plan) { collectPeriods(plan) }
    var selected by remember(plan.id) { mutableStateOf(periods.firstOrNull()) }
    var coupon by rememberSaveable(plan.id) { mutableStateOf("") }
    var tradeNo by rememberSaveable(plan.id) { mutableStateOf<String?>(null) }
    var uncertain by rememberSaveable(plan.id) { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var couponStatus by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    if (tradeNo != null) {
        OrderPaymentSheet(viewModel, tradeNo!!, onDismiss)
        return
    }
    ModalBottomSheet(onDismissRequest = { if (!busy) onDismiss() }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Text(plan.name, style = MaterialTheme.typography.headlineSmall)
            Text("选择周期后生成订单，核对余额抵扣与实付金额，再确认支付。")
            periods.forEach { option ->
                FilterChip(selected = selected == option, onClick = { selected = option; couponStatus = null }, enabled = !busy && !uncertain, label = { Text("${stringResource(option.labelRes)} · ¥${formatYuan(option.priceCents)}") })
            }
            OutlinedTextField(value = coupon, onValueChange = { coupon = it; couponStatus = null }, label = { Text("优惠券（选填）") }, singleLine = true, enabled = !busy && !uncertain, modifier = Modifier.fillMaxWidth())
            if (coupon.isNotBlank()) TextButton(enabled = !busy && !uncertain && selected != null, onClick = {
                val period = selected ?: return@TextButton
                busy = true
                scope.launch {
                    val result = viewModel.checkCoupon(coupon.trim(), plan.id, period.key)
                    couponStatus = if (result == null) "此优惠券无法用于所选周期" else "优惠券有效，优惠金额将在订单中确认"
                    busy = false
                }
            }) { Text("验证当前周期优惠") }
            couponStatus?.let { Text(it, color = MaterialTheme.colorScheme.primary) }
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (uncertain) {
                Text("订单提交结果尚未确认，请到「我的 → 历史订单」查看后再操作，避免重复创建订单。")
                OutlinedButton(onClick = { viewModel.refreshOrders(); onDismiss() }, modifier = Modifier.fillMaxWidth()) { Text("关闭并检查订单") }
            } else Button(enabled = !busy && selected != null && state.enabled("purchase"), modifier = Modifier.fillMaxWidth(), onClick = {
                val period = selected ?: return@Button
                busy = true; error = null
                scope.launch {
                    val trade = viewModel.saveOrder(SaveOrderArgs(plan.id, period.key, coupon.trim().takeIf { it.isNotBlank() }))
                    if (trade.isNullOrBlank()) { uncertain = true; error = "尚未收到订单编号" }
                    else { tradeNo = trade; viewModel.refreshOrders() }
                    busy = false
                }
            }) { Text(if (busy) "处理中…" else "生成订单并核对") }
            TextButton(onClick = onDismiss, enabled = !busy, modifier = Modifier.fillMaxWidth()) { Text("稍后再买") }
        }
    }
}
