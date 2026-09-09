package com.xboard.client.ui.components

import android.annotation.SuppressLint
import android.graphics.Color
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.*
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import com.xboard.client.core.SiteConfig
import kotlinx.coroutines.delay
import org.json.JSONObject
import org.json.JSONTokener
import java.io.ByteArrayInputStream
import java.net.URI

private val challengeHosts = setOf("challenges.cloudflare.com", "www.google.com", "www.recaptcha.net", "recaptcha.net", "www.gstatic.com", "fonts.gstatic.com", "www.gstatic.cn", "www.google.cn")
private data class CaptchaSpec(val kind: String, val key: String, val origin: String?, val error: String?)
private fun captchaSpec(site: SiteConfig?): CaptchaSpec {
    if (site == null) return CaptchaSpec("loading", "", null, "正在读取安全验证配置。若长时间未显示，请刷新配置。")
    if (!site.isCaptcha && !site.isRecaptcha) return CaptchaSpec("none", "", null, null)
    val kind = site.captchaType.ifBlank { if (site.isRecaptcha) "recaptcha" else "" }
    val key = when (kind) { "turnstile" -> site.turnstileSiteKey; "recaptcha", "recaptcha-v2" -> site.recaptchaSiteKey; "recaptcha-v3" -> site.recaptchaV3SiteKey; else -> "" }
    val origin = runCatching {
        val uri = URI(site.appUrl)
        require(uri.scheme.equals("https", true) && !uri.host.isNullOrBlank() && uri.rawUserInfo == null)
        "https://${uri.rawAuthority}"
    }.getOrNull()
    val error = when { key.isBlank() -> "服务方未提供有效的验证码配置，请联系支持团队。"; origin == null -> "服务方未配置安全验证所需的 HTTPS 站点地址，请联系支持团队。"; else -> null }
    return CaptchaSpec(kind, key, origin, error)
}

/** Tokens stay in memory, are consumed once, and are never put in URLs/logs. */
class CaptchaGate internal constructor(internal val kind: String, initialError: String?) {
    var error by mutableStateOf(initialError); internal set
    var busy by mutableStateOf(false); private set
    var ready by mutableStateOf(kind == "none"); internal set
    internal var sequence by mutableIntStateOf(0)
    internal var reset by mutableIntStateOf(0)
    internal var action = "login"
    private var token: String? = null
    private var continuation: ((String?) -> Unit)? = null
    val isTurnstile get() = kind == "turnstile"

    fun request(action: String, onToken: (String?) -> Unit) {
        if (busy) return
        if (kind == "none") { onToken(null); return }
        if (kind == "loading" || !ready) { error = "安全验证尚未就绪，请完成验证或刷新后重试。"; return }
        error = null
        this.action = action
        continuation = onToken
        busy = true
        if (kind == "recaptcha-v3") sequence++
        else token?.let { consume(it) } ?: run { error = "请完成下方安全验证，验证通过后会继续。" }
    }
    internal fun receive(kind: String, value: String?) {
        when (kind) {
            "ready" -> { ready = true; error = null }
            "token" -> if (!value.isNullOrBlank() && value.length <= 16_384) {
                ready = true; token = value; error = null
                if (continuation != null) consume(value)
            }
            "expired" -> { token = null; busy = false; continuation = null; error = "验证已过期，请重新验证。"; reset++ }
            "error" -> { token = null; busy = false; continuation = null; error = "安全验证暂未完成，请重试。" }
        }
    }
    private fun consume(value: String) {
        val callback = continuation
        continuation = null; token = null; busy = false; reset++
        callback?.invoke(value)
    }
    fun retry() { token = null; continuation = null; busy = false; error = null; reset++ }
    internal fun dispose() { continuation = null; token = null; busy = false }
}

@Composable
fun rememberCaptcha(site: SiteConfig?): CaptchaGate {
    val spec = remember(site) { captchaSpec(site) }
    val gate = remember(spec) { CaptchaGate(spec.kind, spec.error) }
    DisposableEffect(gate) { onDispose { gate.dispose() } }
    return gate
}

/** The host only reads a result from its own main document. No JavaScript
 * interface or native capability is exposed to third-party challenge frames. */
@SuppressLint("SetJavaScriptEnabled")
@Composable
fun CaptchaWidget(site: SiteConfig?, gate: CaptchaGate, onReloadConfig: () -> Unit) {
    val spec = remember(site) { captchaSpec(site) }
    if (spec.kind == "none") return
    var webView by remember(spec) { mutableStateOf<WebView?>(null) }
    var height by remember(spec) { mutableIntStateOf(if (spec.kind == "recaptcha-v3") 60 else 110) }
    val pageUrl = "${spec.origin}/__sufe_captcha__/"
    var lastMessage by remember(spec) { mutableIntStateOf(0) }
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text("安全验证", style = MaterialTheme.typography.labelLarge)
        if (spec.error == null) AndroidView(
            modifier = Modifier.fillMaxWidth().height(height.dp),
            factory = { context -> WebView(context).apply {
                setBackgroundColor(Color.TRANSPARENT)
                settings.javaScriptEnabled = true
                settings.domStorageEnabled = true
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
                settings.setSupportMultipleWindows(false)
                settings.mediaPlaybackRequiresUserGesture = true
                isNestedScrollingEnabled = true
                webViewClient = object : WebViewClient() {
                    override fun shouldOverrideUrlLoading(view: WebView?, request: WebResourceRequest): Boolean {
                        val url = request.url.toString()
                        if (request.isForMainFrame) return url != pageUrl
                        return !allowedChallengeUrl(url)
                    }
                    override fun shouldInterceptRequest(view: WebView?, request: WebResourceRequest): WebResourceResponse? {
                        if (request.url.toString() == pageUrl || allowedChallengeUrl(request.url.toString())) return null
                        return WebResourceResponse("text/plain", "UTF-8", ByteArrayInputStream(ByteArray(0)))
                    }
                }
                loadDataWithBaseURL(pageUrl, captchaHtml(spec), "text/html", "UTF-8", null)
                webView = this
            } },
        )
        (spec.error ?: gate.error)?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
        TextButton(onClick = {
            gate.retry()
            if (spec.error != null || !gate.ready) {
                onReloadConfig()
                lastMessage = 0
                webView?.loadDataWithBaseURL(pageUrl, captchaHtml(spec), "text/html", "UTF-8", null)
            }
        }) { Text("重新验证") }
    }
    LaunchedEffect(webView) {
        val web = webView ?: return@LaunchedEffect
        while (true) {
            delay(350)
            web.evaluateJavascript("JSON.stringify(window.__sufeResult || null)") { encoded ->
                val value = runCatching { JSONTokener(encoded).nextValue() as? String }.getOrNull() ?: return@evaluateJavascript
                val message = runCatching { JSONObject(value) }.getOrNull() ?: return@evaluateJavascript
                val number = message.optInt("sequence")
                if (number > lastMessage) {
                    lastMessage = number
                    gate.receive(message.optString("kind"), message.optString("token"))
                }
            }
            web.evaluateJavascript("Math.max(90,Math.min(540,Array.from(document.querySelectorAll('iframe')).reduce((n,f)=>{const r=f.getBoundingClientRect();return r.width>0&&r.height>0?Math.max(n,r.bottom):n},0)))") { value ->
                value.toIntOrNull()?.let { if (spec.kind != "recaptcha-v3") height = it }
            }
        }
    }
    LaunchedEffect(gate.busy, gate.sequence) {
        if (gate.busy) { delay(60_000); if (gate.busy) gate.receive("error", null) }
    }
    LaunchedEffect(gate.sequence) {
        if (gate.sequence > 0) webView?.evaluateJavascript("window.__sufeExecute && window.__sufeExecute(${JSONObject.quote(gate.action)})", null)
    }
    LaunchedEffect(gate.reset) {
        if (gate.reset > 0) webView?.evaluateJavascript("window.__sufeReset && window.__sufeReset()", null)
    }
    DisposableEffect(webView) {
        val web = webView
        onDispose { web?.stopLoading(); web?.destroy() }
    }
}

private fun allowedChallengeUrl(url: String): Boolean {
    if (url == "about:blank" || url == "about:srcdoc" || url.startsWith("data:image/")) return true
    return runCatching { URI(url).let { it.scheme.equals("https", true) && it.host?.lowercase() in challengeHosts && it.rawUserInfo == null } }.getOrDefault(false)
}
private fun captchaHtml(spec: CaptchaSpec): String {
    val key = JSONObject.quote(spec.key).replace("<", "\\u003c")
    val callback = "function report(kind,token){window.__sufeResult={sequence:++n,kind:kind,token:token||''};}"
    val script = when (spec.kind) {
        "turnstile" -> """
            window.loaded=function(){widget=turnstile.render('#challenge',{sitekey:$key,theme:'light',callback:t=>report('token',t),'expired-callback':()=>report('expired'),'error-callback':()=>report('error')});report('ready');};
            window.__sufeReset=function(){if(widget!==null)turnstile.reset(widget);};
        """.trimIndent()
        "recaptcha-v3" -> """
            window.loaded=function(){grecaptcha.ready(()=>report('ready'));};
            window.__sufeExecute=function(action){grecaptcha.ready(()=>grecaptcha.execute($key,{action:action}).then(t=>report('token',t)).catch(()=>report('error')));};
            window.__sufeReset=function(){};
        """.trimIndent()
        else -> """
            window.loaded=function(){widget=grecaptcha.render('challenge',{sitekey:$key,callback:t=>report('token',t),'expired-callback':()=>report('expired'),'error-callback':()=>report('error')});report('ready');};
            window.__sufeReset=function(){if(widget!==null)grecaptcha.reset(widget);};
        """.trimIndent()
    }
    val url = when (spec.kind) {
        "turnstile" -> "https://challenges.cloudflare.com/turnstile/v0/api.js?onload=loaded&render=explicit"
        "recaptcha-v3" -> "https://www.recaptcha.net/recaptcha/api.js?onload=loaded&render=${java.net.URLEncoder.encode(spec.key, "UTF-8")}" 
        else -> "https://www.recaptcha.net/recaptcha/api.js?onload=loaded&render=explicit"
    }
    return """<!doctype html><html><head><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' https://challenges.cloudflare.com https://www.recaptcha.net https://www.google.com https://www.gstatic.com; frame-src 'self' about: https://challenges.cloudflare.com https://www.recaptcha.net https://www.google.com; connect-src https://challenges.cloudflare.com https://www.recaptcha.net https://www.google.com https://www.gstatic.com; style-src 'unsafe-inline'; img-src data: https://challenges.cloudflare.com https://www.gstatic.com https://www.google.com https://www.recaptcha.net; worker-src blob:;"><style>body{margin:0;background:transparent;font-family:system-ui}#challenge{padding:4px}</style></head><body><div id="challenge"></div><script>var n=0,widget=null;$callback $script</script><script async defer src="$url" onerror="report('error')"></script></body></html>"""
}
