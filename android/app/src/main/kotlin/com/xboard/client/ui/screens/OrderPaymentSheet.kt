package com.xboard.client.ui.screens

import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.zxing.BarcodeFormat
import com.google.zxing.MultiFormatWriter
import com.xboard.client.core.PaymentMethod
import com.xboard.client.util.formatYuan
import com.xboard.client.vm.AppViewModel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject
import org.json.JSONTokener
import java.math.RoundingMode

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun OrderPaymentSheet(viewModel: AppViewModel, tradeNo: String, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val lifecycle = LocalLifecycleOwner.current
    var order by remember(tradeNo) { mutableStateOf<JSONObject?>(null) }
    var methods by remember(tradeNo) { mutableStateOf<List<PaymentMethod>>(emptyList()) }
    var methodId by remember(tradeNo) { mutableStateOf<Long?>(null) }
    var busy by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var status by remember { mutableStateOf<Int?>(null) }
    var polling by remember { mutableStateOf(false) }
    var payload by remember { mutableStateOf<String?>(null) }
    var checkoutStarted by remember { mutableStateOf(false) }
    var qr by remember { mutableStateOf(false) }
    var checking by remember { mutableStateOf(false) }
    val total = order?.optLong("total_amount") ?: 0L
    val bound = order?.optLong("payment_id")?.takeIf { it > 0 }
    val selected = methods.find { it.id == (bound ?: methodId) }
    val calculatedFee = selected?.let {
        (total.toBigDecimal() * (it.handlingFeePercent ?: 0.0).toBigDecimal() / 100.toBigDecimal()).setScale(0, RoundingMode.HALF_UP).toLong() + (it.handlingFeeFixed ?: 0L)
    } ?: 0L
    val fee = if (bound != null && order?.has("handling_amount") == true && order?.isNull("handling_amount") == false) order?.optLong("handling_amount") ?: 0L else calculatedFee

    suspend fun refreshStatus() {
        if (checking || status in listOf(2, 3, 4)) return
        checking = true
        try {
        val next = viewModel.checkOrder(tradeNo)
        if (next == null) { polling = false; error = "暂时无法确认订单状态，请手动重试"; return }
        status = next
        if (next == 2 || next == 3 || next == 4) {
            polling = false
            payload = null
            viewModel.refreshOrders()
            if (next == 3 || next == 4) viewModel.refreshHome()
        }
        } finally { checking = false }
    }
    suspend fun load() {
        busy = true; error = null
        val detail = viewModel.orderDetails(tradeNo)
        if (detail == null) { error = "订单详情加载失败，可重试或稍后从历史订单继续"; busy = false; return }
        order = detail; status = detail.optInt("status")
        if (detail.optLong("total_amount") > 0) methods = viewModel.paymentMethods() ?: emptyList()
        methodId = detail.optLong("payment_id").takeIf { it > 0 } ?: methods.firstOrNull()?.id
        if (status == 1) polling = true
        busy = false
    }
    LaunchedEffect(tradeNo) { load() }
    LaunchedEffect(polling, tradeNo) {
        if (!polling) return@LaunchedEffect
        val deadline = android.os.SystemClock.elapsedRealtime() + 600_000
        while (android.os.SystemClock.elapsedRealtime() < deadline) {
            delay(5_000)
            refreshStatus()
            if (!polling) return@LaunchedEffect
        }
        polling = false; error = "查询已暂停，您可以手动刷新或稍后在历史订单查看结果"
    }
    DisposableEffect(lifecycle, checkoutStarted) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME && checkoutStarted) scope.launch { refreshStatus() }
        }
        lifecycle.lifecycle.addObserver(observer)
        onDispose { lifecycle.lifecycle.removeObserver(observer) }
    }
    ModalBottomSheet(onDismissRequest = { if (!busy) onDismiss() }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Text(when (status) { 1 -> "支付已确认，正在开通"; 2 -> "订单已取消"; 3, 4 -> "订单已完成"; else -> "核对订单并支付" }, style = MaterialTheme.typography.headlineSmall)
            Text(tradeNo, style = MaterialTheme.typography.labelMedium)
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (order == null && !busy) Button(onClick = { scope.launch { load() } }) { Text("重新加载订单") }
            order?.let { detail ->
                detail.optJSONObject("plan")?.optString("name")?.let { Text(it) }
                Text("余额抵扣：¥${formatYuan(detail.optLong("balance_amount"))}")
                Text("优惠抵扣：¥${formatYuan(detail.optLong("discount_amount"))}")
                Text("订单应付：¥${formatYuan(total)}", style = MaterialTheme.typography.titleMedium)
                if (status == 0 && payload == null) {
                    if (total > 0) {
                        Text("选择支付方式")
                        if (methods.isEmpty() && bound == null) Text("暂无可用支付渠道，请稍后重试或联系客服。")
                        methods.forEach { method -> FilterChip(selected = (bound ?: methodId) == method.id, enabled = !busy && bound == null && !checkoutStarted, onClick = { methodId = method.id }, label = { Text(method.name) }) }
                        if (bound != null) Text("已保留原订单支付渠道；手续费以订单记录为准。", style = MaterialTheme.typography.bodySmall)
                        Text("渠道手续费：¥${formatYuan(fee)}")
                    } else Text("无需外部支付，确认后即可等待套餐开通。")
                    Text("实付：¥${formatYuan(total + if (total > 0) fee else 0L)}", style = MaterialTheme.typography.headlineSmall)
                    Button(enabled = !busy && (total == 0L || (bound ?: methodId) != null), modifier = Modifier.fillMaxWidth(), onClick = {
                        busy = true; error = null
                        scope.launch {
                            val response = viewModel.checkoutOrder(tradeNo, if (total == 0L) 0L else bound ?: methodId ?: 0L)
                            checkoutStarted = true
                            if (response == null) {
                                error = "支付结果未确认，订单已保留。请先查询状态后再继续支付。"
                                val snapshot = viewModel.orderDetails(tradeNo)
                                if (snapshot != null) { order = snapshot; status = snapshot.optInt("status") }
                            } else {
                                payload = paymentPayload(response.dataJson)
                                qr = response.kind == 0
                                if (response.kind !in listOf(-1, 0, 1, -2) || (response.kind != -1 && payload == null)) error = "支付渠道未返回可使用的支付信息，请联系客服。"
                                polling = true
                                refreshStatus()
                            }
                            busy = false
                        }
                    }) { Text(if (busy) "处理中…" else if (checkoutStarted) "继续此订单付款" else "确认支付") }
                }
            }
            payload?.let { value ->
                if (qr) {
                    val bitmap = remember(value) { paymentQr(value) }
                    if (bitmap != null) Image(bitmap.asImageBitmap(), contentDescription = "支付二维码", modifier = Modifier.size(240.dp).align(Alignment.CenterHorizontally))
                }
                val uri = remember(value) { runCatching { Uri.parse(value) }.getOrNull() }
                val supported = uri?.scheme?.lowercase() in listOf("https", "http", "alipays", "weixin")
                if (supported) Button(onClick = {
                    runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, uri)) }.onFailure { error = "无法打开支付应用，请复制支付信息后继续" }
                }, modifier = Modifier.fillMaxWidth()) { Text("打开支付页面") }
                OutlinedButton(onClick = { viewModel.emitClipboardCopy(value, "支付信息已复制") }, modifier = Modifier.fillMaxWidth()) { Text("复制支付信息") }
            }
            if (order != null && status !in listOf(2, 3, 4)) OutlinedButton(onClick = { scope.launch { refreshStatus() } }, enabled = !busy, modifier = Modifier.fillMaxWidth()) { Text(if (polling) "正在自动查询 · 手动刷新" else "查询最新状态") }
            TextButton(onClick = { viewModel.refreshOrders(); onDismiss() }, enabled = !busy, modifier = Modifier.fillMaxWidth()) { Text("关闭，稍后从订单继续") }
        }
    }
}

private fun paymentPayload(json: String?): String? {
    if (json.isNullOrBlank()) return null
    return when (val value = runCatching { JSONTokener(json).nextValue() }.getOrNull()) {
        is String -> value.takeIf { it.isNotBlank() }
        is JSONObject -> listOf("url", "redirect", "qr_code", "qrcode").firstNotNullOfOrNull { key -> value.optString(key).takeIf { it.isNotBlank() && it != "null" } }
        else -> null
    }
}
private fun paymentQr(value: String): Bitmap? = runCatching {
    val matrix = MultiFormatWriter().encode(value, BarcodeFormat.QR_CODE, 400, 400)
    val pixels = IntArray(400 * 400) { i -> if (matrix[i % 400, i / 400]) android.graphics.Color.BLACK else android.graphics.Color.WHITE }
    Bitmap.createBitmap(pixels, 400, 400, Bitmap.Config.ARGB_8888)
}.getOrNull()
