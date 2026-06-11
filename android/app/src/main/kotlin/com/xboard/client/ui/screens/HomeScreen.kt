package com.xboard.client.ui.screens

import android.graphics.Paint
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.PowerSettingsNew
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
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
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.xboard.client.R
import com.xboard.client.core.ConnectionState
import com.xboard.client.core.ProxyGroup
import com.xboard.client.core.SubscribeInfo
import com.xboard.client.core.UserInfo
import com.xboard.client.ui.components.ScreenScaffold
import com.xboard.client.ui.components.ScrollableColumn
import com.xboard.client.ui.components.TopProgressStrip
import com.xboard.client.util.formatBytes
import com.xboard.client.util.formatDate
import com.xboard.client.util.formatYuan
import com.xboard.client.vm.AppViewModel
import com.xboard.client.vpn.VpnBinder
import kotlin.math.PI
import kotlin.math.ln
import kotlin.math.max
import kotlin.math.min
import kotlin.math.tan

@Composable
fun HomeScreen(
    viewModel: AppViewModel,
    vpnBinder: VpnBinder,
    onOpenConnect: () -> Unit,
    onOpenPlans: () -> Unit,
    onOpenOrders: () -> Unit,
    onOpenTickets: () -> Unit,
    onOpenNotices: () -> Unit,
) {
    val home by viewModel.home.collectAsState()
    val connection by viewModel.connection.collectAsState()
    val ctx = LocalContext.current
    val pins = remember(connection.proxies, connection.selectedNode) {
        collectMapPins(connection.proxies, connection.selectedNode)
    }
    val selectedLocation = remember(connection.selectedNode) {
        connection.selectedNode?.let(::locateNode)
    }
    val connected = connection.state is ConnectionState.Connected
    val connecting = connection.state is ConnectionState.Connecting

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
        Box(
            modifier = padded
                .background(
                    Brush.verticalGradient(
                        listOf(Color(0xFF21182D), Color(0xFF14111C), Color(0xFF10141E)),
                    ),
                ),
        ) {
            WorldMapCanvas(
                pins = pins,
                modifier = Modifier.fillMaxSize(),
            )
            TopProgressStrip(visible = home.refreshing)

            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(horizontal = 20.dp, vertical = 18.dp),
                verticalArrangement = Arrangement.SpaceBetween,
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth()) {
                        Text(
                            text = if (connected) "已保护" else "未保护",
                            color = if (connected) Color(0xFF00D09C) else Color(0xFFFF6380),
                            style = MaterialTheme.typography.headlineSmall,
                            fontWeight = FontWeight.ExtraBold,
                        )
                        Text(
                            text = if (connected) {
                                connection.selectedNode ?: stringResource(R.string.connect_current_node)
                            } else {
                                "连接以保护您的隐私"
                            },
                            color = Color(0xFFC9C1D8),
                            style = MaterialTheme.typography.bodyMedium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }

                    home.subscribe?.let { info ->
                        ProtonTrafficCard(
                            sub = info,
                            plan = selectedLocation?.let { "${it.flag} ${it.country} · ${it.label}" },
                            onCopy = {
                                viewModel.emitClipboardCopy(
                                    text = info.subscribeUrl,
                                    toastMessage = ctx.getString(R.string.home_copied),
                                )
                            },
                        )
                    }
                }

                Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    SelectedServerCard(
                        node = connection.selectedNode,
                        route = connection.selectedRoute,
                        location = selectedLocation,
                        connected = connected,
                        connecting = connecting,
                        onPickNode = onOpenConnect,
                        onToggle = {
                            if (connected) viewModel.disconnect() else viewModel.connect(vpnBinder)
                        },
                    )
                    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            DashboardAction("节点", onOpenConnect, Modifier.weight(1f))
                            DashboardAction(stringResource(R.string.home_menu_plans), onOpenPlans, Modifier.weight(1f))
                            DashboardAction(stringResource(R.string.home_menu_tickets), onOpenTickets, Modifier.weight(1f))
                        }
                        Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            DashboardAction(stringResource(R.string.home_menu_orders), onOpenOrders, Modifier.weight(1f))
                            DashboardAction(stringResource(R.string.home_menu_notices), onOpenNotices, Modifier.weight(1f))
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ProtonTrafficCard(sub: SubscribeInfo, plan: String?, onCopy: () -> Unit) {
    val total = sub.transferEnable
    val used = sub.upload + sub.download
    val ratio = if (total == 0UL) 0f else (used.toFloat() / total.toFloat()).coerceIn(0f, 1f)
    Surface(
        color = Color(0xCC1C1726.toInt()),
        shape = RoundedCornerShape(20.dp),
        border = BorderStroke(1.dp, Color.White.copy(alpha = 0.08f)),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(modifier = Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Surface(shape = RoundedCornerShape(8.dp), color = Color(0xFF006D52)) {
                        Icon(
                            Icons.Default.Bolt,
                            contentDescription = null,
                            tint = Color(0xFF00E0A7),
                            modifier = Modifier.padding(7.dp).size(18.dp),
                        )
                    }
                    Text("流量使用", color = Color.White, fontWeight = FontWeight.Bold)
                }
                IconButton(onClick = onCopy) {
                    Icon(Icons.Default.ContentCopy, contentDescription = stringResource(R.string.home_copy), tint = Color(0xFFC9C1D8))
                }
            }
            LinearProgressIndicator(
                progress = { ratio },
                modifier = Modifier.fillMaxWidth().height(8.dp).clip(CircleShape),
                color = Color(0xFF7C5CFF),
                trackColor = Color.White.copy(alpha = 0.12f),
            )
            Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text("${stringResource(R.string.home_used)} ${formatBytes(used)}", color = Color(0xFFEDE8F7))
                Text("${stringResource(R.string.home_total)} ${formatBytes(total)}", color = Color(0xFFEDE8F7))
            }
            plan?.let {
                Text(it, color = Color(0xFF00D09C), style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun SelectedServerCard(
    node: String?,
    route: String?,
    location: GeoPoint?,
    connected: Boolean,
    connecting: Boolean,
    onPickNode: () -> Unit,
    onToggle: () -> Unit,
) {
    Surface(
        color = Color(0xDD171320),
        shape = RoundedCornerShape(22.dp),
        border = BorderStroke(1.dp, Color.White.copy(alpha = 0.1f)),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(modifier = Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Text("当前选择", color = Color(0xFF9E96AE), style = MaterialTheme.typography.labelLarge)
            Row(
                modifier = Modifier.fillMaxWidth().clickable(onClick = onPickNode),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = node ?: "最快服务器",
                        color = Color.White,
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.ExtraBold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = route ?: location?.let { "${it.flag} ${it.country} · ${it.label}" } ?: "自动选择最优节点",
                        color = Color(0xFFC9C1D8),
                        style = MaterialTheme.typography.bodySmall,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                Text("›", color = Color(0xFFC9C1D8), style = MaterialTheme.typography.headlineSmall)
            }
            Surface(
                onClick = { if (!connecting) onToggle() },
                enabled = !connecting,
                shape = RoundedCornerShape(14.dp),
                color = Color.Transparent,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(58.dp)
                    .background(
                        Brush.horizontalGradient(listOf(Color(0xFF7C5CFF), Color(0xFF00A978))),
                        RoundedCornerShape(14.dp),
                    ),
            ) {
                Row(horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Default.PowerSettingsNew, contentDescription = null, tint = Color.White)
                    Spacer(Modifier.width(8.dp))
                    Text(
                        text = when {
                            connecting -> "连接中"
                            connected -> "断开连接"
                            else -> "快速连接"
                        },
                        color = Color.White,
                        fontWeight = FontWeight.ExtraBold,
                    )
                }
            }
        }
    }
}

@Composable
private fun DashboardAction(label: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    Surface(
        modifier = modifier.height(48.dp),
        onClick = onClick,
        shape = RoundedCornerShape(14.dp),
        color = Color.White.copy(alpha = 0.08f),
        border = BorderStroke(1.dp, Color.White.copy(alpha = 0.08f)),
    ) {
        Box(contentAlignment = Alignment.Center) {
            Text(label, color = Color.White, fontWeight = FontWeight.Bold)
        }
    }
}

@Composable
private fun WorldMapCanvas(pins: List<MapPin>, modifier: Modifier = Modifier) {
    Canvas(modifier = modifier) {
        val w = size.width
        val h = size.height
        val stroke = Stroke(width = 1.2.dp.toPx())
        val landColor = Color(0x66100D16)
        val lineColor = Color.White.copy(alpha = 0.18f)

        fun path(points: List<Pair<Float, Float>>) = Path().apply {
            points.firstOrNull()?.let { moveTo(it.first * w, it.second * h) }
            points.drop(1).forEach { lineTo(it.first * w, it.second * h) }
            close()
        }

        drawPath(
            path(listOf(0.45f to 0.18f, 0.58f to 0.12f, 0.72f to 0.20f, 0.88f to 0.28f, 0.82f to 0.48f, 0.67f to 0.52f, 0.56f to 0.62f, 0.42f to 0.56f, 0.32f to 0.42f)),
            landColor,
            style = stroke,
        )
        drawPath(
            path(listOf(0.17f to 0.18f, 0.30f to 0.25f, 0.27f to 0.45f, 0.20f to 0.62f, 0.12f to 0.45f, 0.10f to 0.27f)),
            landColor,
            style = stroke,
        )
        drawPath(
            path(listOf(0.75f to 0.66f, 0.86f to 0.72f, 0.82f to 0.84f, 0.72f to 0.78f)),
            landColor,
            style = stroke,
        )
        drawLine(lineColor, Offset(w * 0.18f, h * 0.54f), Offset(w * 0.86f, h * 0.36f), strokeWidth = 1.dp.toPx())
        drawLine(lineColor, Offset(w * 0.42f, h * 0.62f), Offset(w * 0.82f, h * 0.44f), strokeWidth = 1.dp.toPx())

        pins.forEach { pin ->
            val x = w * pin.x / 100f
            val y = h * pin.y / 100f
            val color = if (pin.active) Color(0xFF00D09C) else Color(0xFFFF4057)
            drawCircle(color.copy(alpha = 0.20f), radius = if (pin.active) 22.dp.toPx() else 15.dp.toPx(), center = Offset(x, y))
            drawCircle(color, radius = if (pin.active) 6.dp.toPx() else 4.dp.toPx(), center = Offset(x, y))
            if (pin.active) {
                drawContext.canvas.nativeCanvas.drawText(
                    pin.flag,
                    x + 8.dp.toPx(),
                    y - 8.dp.toPx(),
                    Paint().apply {
                        isAntiAlias = true
                        textSize = 22.dp.toPx()
                        // Qualify with `this`: the enclosing forEach defines a
                        // local `val color: Color` (Compose) that otherwise
                        // shadows android.graphics.Paint.color here.
                        this.color = android.graphics.Color.WHITE
                    },
                )
            }
        }
    }
}

private data class GeoPoint(
    val label: String,
    val country: String,
    val flag: String,
    val lat: Double,
    val lon: Double,
    val keys: List<String>,
)

private data class MapPin(
    val label: String,
    val country: String,
    val flag: String,
    val x: Float,
    val y: Float,
    val count: Int,
    val active: Boolean,
)

private val geoAliases = listOf(
    GeoPoint("Taipei", "台湾", "🇹🇼", 25.033, 121.565, listOf("台北", "taipei")),
    GeoPoint("Taiwan", "台湾", "🇹🇼", 23.697, 120.961, listOf("台湾", "台灣", "taiwan", " tw")),
    GeoPoint("Tokyo", "日本", "🇯🇵", 35.676, 139.650, listOf("东京", "東京", "tokyo")),
    GeoPoint("Osaka", "日本", "🇯🇵", 34.694, 135.502, listOf("大阪", "osaka")),
    GeoPoint("Japan", "日本", "🇯🇵", 36.204, 138.253, listOf("日本", "japan", " jp")),
    GeoPoint("Hong Kong", "香港", "🇭🇰", 22.319, 114.169, listOf("香港", "hong kong", "hongkong", " hk")),
    GeoPoint("Singapore", "新加坡", "🇸🇬", 1.352, 103.820, listOf("新加坡", "singapore", " sg")),
    GeoPoint("Seoul", "韩国", "🇰🇷", 37.566, 126.978, listOf("首尔", "首爾", "seoul")),
    GeoPoint("Korea", "韩国", "🇰🇷", 36.500, 127.800, listOf("韩国", "韓國", "korea", " kr")),
    GeoPoint("Los Angeles", "美国", "🇺🇸", 34.052, -118.244, listOf("洛杉矶", "洛杉磯", "los angeles", "la-")),
    GeoPoint("San Jose", "美国", "🇺🇸", 37.338, -121.886, listOf("圣何塞", "聖何塞", "san jose", "sanjose")),
    GeoPoint("New York", "美国", "🇺🇸", 40.713, -74.006, listOf("纽约", "紐約", "new york")),
    GeoPoint("United States", "美国", "🇺🇸", 39.828, -98.579, listOf("美国", "美國", "united states", "usa", " us")),
    GeoPoint("London", "英国", "🇬🇧", 51.507, -0.128, listOf("伦敦", "倫敦", "london")),
    GeoPoint("United Kingdom", "英国", "🇬🇧", 54.000, -2.000, listOf("英国", "英國", "united kingdom", " uk")),
    GeoPoint("Frankfurt", "德国", "🇩🇪", 50.110, 8.682, listOf("法兰克福", "法蘭克福", "frankfurt")),
    GeoPoint("Germany", "德国", "🇩🇪", 51.165, 10.452, listOf("德国", "德國", "germany", " de")),
    GeoPoint("Paris", "法国", "🇫🇷", 48.857, 2.352, listOf("巴黎", "paris", "法国", "法國", "france")),
    GeoPoint("Amsterdam", "荷兰", "🇳🇱", 52.367, 4.904, listOf("阿姆斯特丹", "amsterdam", "荷兰", "荷蘭", "netherlands", " nl")),
    GeoPoint("Sydney", "澳大利亚", "🇦🇺", -33.869, 151.209, listOf("悉尼", "sydney")),
    GeoPoint("Australia", "澳大利亚", "🇦🇺", -25.274, 133.775, listOf("澳大利亚", "澳洲", "australia", " au")),
    GeoPoint("Toronto", "加拿大", "🇨🇦", 43.653, -79.383, listOf("多伦多", "多倫多", "toronto")),
    GeoPoint("Canada", "加拿大", "🇨🇦", 56.130, -106.347, listOf("加拿大", "canada", " ca")),
    GeoPoint("Bangkok", "泰国", "🇹🇭", 13.756, 100.501, listOf("曼谷", "bangkok", "泰国", "泰國", "thailand")),
    GeoPoint("Ho Chi Minh City", "越南", "🇻🇳", 10.823, 106.630, listOf("胡志明", "越南", "vietnam", " vn")),
    GeoPoint("Manila", "菲律宾", "🇵🇭", 14.599, 120.984, listOf("马尼拉", "馬尼拉", "菲律宾", "菲律賓", "philippines")),
    GeoPoint("Kuala Lumpur", "马来西亚", "🇲🇾", 3.139, 101.687, listOf("吉隆坡", "马来", "馬來", "malaysia")),
    GeoPoint("Jakarta", "印尼", "🇮🇩", -6.208, 106.846, listOf("雅加达", "雅加達", "印尼", "indonesia")),
    GeoPoint("Mumbai", "印度", "🇮🇳", 19.076, 72.878, listOf("孟买", "孟買", "mumbai")),
    GeoPoint("India", "印度", "🇮🇳", 20.594, 78.963, listOf("印度", "india")),
    GeoPoint("Dubai", "阿联酋", "🇦🇪", 25.205, 55.271, listOf("迪拜", "dubai", "阿联酋", "阿聯酋", "uae")),
    GeoPoint("Istanbul", "土耳其", "🇹🇷", 41.008, 28.978, listOf("伊斯坦布尔", "土耳其", "turkey", "istanbul")),
    GeoPoint("Moscow", "俄罗斯", "🇷🇺", 55.756, 37.617, listOf("莫斯科", "俄罗斯", "俄羅斯", "russia")),
    GeoPoint("Shanghai", "中国", "🇨🇳", 31.231, 121.474, listOf("上海", "shanghai")),
    GeoPoint("Beijing", "中国", "🇨🇳", 39.904, 116.407, listOf("北京", "beijing", "中国", "中國", "china")),
    GeoPoint("Macau", "澳门", "🇲🇴", 22.199, 113.544, listOf("澳门", "澳門", "macau", "macao")),
)

private fun locateNode(name: String): GeoPoint? {
    val normalized = " " + name.lowercase()
        .replace(Regex("[|｜_·•/()\\[\\]{}]+"), " ")
        .replace(Regex("\\s+"), " ") + " "
    return geoAliases.firstOrNull { point ->
        point.keys.any { normalized.contains(it.lowercase()) }
    }
}

private fun collectMapPins(groups: List<ProxyGroup>, activeNode: String?): List<MapPin> {
    val counters = linkedMapOf<String, Pair<GeoPoint, Int>>()
    val names = buildList {
        if (activeNode != null) add(activeNode)
        groups.forEach { group -> addAll(group.all) }
    }
    names.forEach { name ->
        val loc = locateNode(name) ?: return@forEach
        val key = "${loc.lat}:${loc.lon}"
        val old = counters[key]
        counters[key] = loc to ((old?.second ?: 0) + 1)
    }
    return counters.map { (_, pair) ->
        val (loc, count) = pair
        val projected = projectGeo(loc.lat, loc.lon)
        MapPin(
            label = loc.label,
            country = loc.country,
            flag = loc.flag,
            x = projected.first,
            y = projected.second,
            count = count,
            active = activeNode?.let { locateNode(it)?.label == loc.label } == true,
        )
    }.sortedWith(compareByDescending<MapPin> { it.active }.thenByDescending { it.count }).take(28)
}

private fun projectGeo(lat: Double, lon: Double): Pair<Float, Float> {
    val x = ((lon + 180.0) / 360.0 * 100.0).coerceIn(2.0, 98.0)
    val clampedLat = max(-85.0, min(85.0, lat))
    val latRad = clampedLat * PI / 180.0
    val merc = ln(tan(PI / 4.0 + latRad / 2.0))
    val y = ((1.0 - merc / PI) / 2.0 * 100.0).coerceIn(6.0, 94.0)
    return x.toFloat() to y.toFloat()
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
