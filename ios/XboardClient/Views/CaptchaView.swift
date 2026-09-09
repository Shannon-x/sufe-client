import SwiftUI
import Observation
import WebKit

/// Tokens live only in this view's memory. Each form request consumes one.
@Observable
@MainActor
final class CaptchaController {
    private(set) var ready = false
    private(set) var busy = false
    private(set) var error: String?
    private(set) var token: String?
    var height: CGFloat = 92
    var revision = 0
    @ObservationIgnored private weak var webView: WKWebView?
    @ObservationIgnored private var provider = ""
    @ObservationIgnored private var expiresAt = Date.distantPast
    @ObservationIgnored private var pending: CheckedContinuation<String?, Error>?
    @ObservationIgnored private var timeoutTask: Task<Void, Never>?
    @ObservationIgnored private var expiryTask: Task<Void, Never>?

    var canSubmit: Bool {
        ready && !busy && (provider == "recaptcha-v3" || (token != nil && expiresAt > Date()))
    }

    func attach(_ webView: WKWebView, provider: String) {
        detach()
        self.webView = webView
        self.provider = provider
        height = 92
        error = nil
        timeoutTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(30))
            guard !Task.isCancelled, let self, !self.ready else { return }
            self.fail("验证码加载超时，请检查网络后重试")
        }
    }

    func detach() {
        timeoutTask?.cancel()
        expiryTask?.cancel()
        timeoutTask = nil
        expiryTask = nil
        pending?.resume(throwing: CaptchaFailure(message: "验证已取消"))
        pending = nil
        webView = nil
        ready = false
        busy = false
        token = nil
        expiresAt = .distantPast
    }

    func resolve(required: Bool, action: String) async throws -> String? {
        guard required else { return nil }
        guard ready, !busy, let webView else {
            throw CaptchaFailure(message: error ?? "请等待验证码加载完成")
        }
        if provider != "recaptcha-v3" {
            guard let token, expiresAt > Date() else {
                throw CaptchaFailure(message: "请先完成安全验证")
            }
            self.token = nil
            expiresAt = .distantPast
            expiryTask?.cancel()
            busy = true
            return token
        }
        guard ["login", "register", "send_email", "reset_password"].contains(action) else {
            throw CaptchaFailure(message: "验证码操作无效")
        }
        busy = true
        error = nil
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                pending = continuation
                // action is an exact member of the fixed native allowlist.
                webView.evaluateJavaScript("window.sufeExecute('\(action)')") { [weak self] _, failure in
                    if failure != nil { self?.fail("无法启动安全验证，请重试") }
                }
                timeoutTask?.cancel()
                timeoutTask = Task { [weak self] in
                    try? await Task.sleep(for: .seconds(25))
                    guard !Task.isCancelled else { return }
                    self?.fail("安全验证超时，请重试")
                }
            }
        } onCancel: {
            Task { @MainActor [weak self] in self?.fail("验证已取消") }
        }
    }

    func reset() {
        token = nil
        expiresAt = .distantPast
        expiryTask?.cancel()
        error = nil
        busy = false
        pending?.resume(throwing: CaptchaFailure(message: "验证已重置"))
        pending = nil
        timeoutTask?.cancel()
        if ready {
            webView?.evaluateJavaScript("window.sufeReset()", completionHandler: nil)
        }
    }

    func reload() {
        detach()
        error = nil
        revision += 1
    }

    func receive(_ kind: String, value: String?) {
        switch kind {
        case "ready":
            ready = true
            error = nil
            timeoutTask?.cancel()
        case "token":
            guard let value, !value.isEmpty, value.utf8.count <= 16_384 else {
                fail("验证码响应无效，请重新验证")
                return
            }
            token = value
            error = nil
            busy = false
            timeoutTask?.cancel()
            let lifetime = provider == "turnstile" ? 270.0 : 110.0
            expiresAt = Date().addingTimeInterval(lifetime)
            expiryTask?.cancel()
            expiryTask = Task { [weak self] in
                try? await Task.sleep(for: .seconds(lifetime))
                guard !Task.isCancelled else { return }
                self?.receive("expired", value: nil)
            }
            pending?.resume(returning: value)
            pending = nil
        case "expired":
            fail("验证码已过期，请重新验证")
        case "error":
            fail("验证未完成，请重试；若持续失败，请联系管理员核对验证码域名")
        default:
            break
        }
    }

    func fail(_ message: String) {
        token = nil
        expiresAt = .distantPast
        busy = false
        error = message
        timeoutTask?.cancel()
        expiryTask?.cancel()
        pending?.resume(throwing: CaptchaFailure(message: message))
        pending = nil
    }
}

struct CaptchaFailure: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

@MainActor
struct AuthSecuritySection: View {
    let siteConfig: SiteConfig?
    let controller: CaptchaController
    let loading: Bool
    let retry: () -> Void

    var body: some View {
        if siteConfig != nil {
            CaptchaView(siteConfig: siteConfig, controller: controller)
        } else {
            HStack(spacing: 10) {
                if loading { ProgressView().controlSize(.small) }
                Text(loading ? "正在加载安全设置…" : "安全设置未加载，暂时无法提交")
                    .font(.footnote).foregroundStyle(.secondary)
                Spacer()
                if !loading { Button("重试", action: retry).font(.footnote) }
            }
            .padding(12)
            .background(Color.white, in: RoundedRectangle(cornerRadius: 12))
        }
    }
}

private struct CaptchaConfiguration: Equatable {
    let provider: String
    let siteKey: String
    let baseURL: URL

    init(_ site: SiteConfig) throws {
        provider = site.captchaType
        switch provider {
        case "turnstile": siteKey = site.turnstileSiteKey
        case "recaptcha": siteKey = site.recaptchaSiteKey
        case "recaptcha-v3": siteKey = site.recaptchaV3SiteKey
        default: throw CaptchaFailure(message: "此验证码类型尚不受支持，请联系客户支持")
        }
        guard !siteKey.isEmpty, siteKey.utf8.count <= 256,
              siteKey.utf8.allSatisfy({ (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || $0 == 45 || $0 == 95 }) else {
            throw CaptchaFailure(message: "验证码站点密钥未配置，请联系管理员")
        }
        guard var parts = URLComponents(string: site.appUrl),
              parts.scheme?.lowercased() == "https", let host = parts.host, !host.isEmpty,
              parts.user == nil, parts.password == nil else {
            throw CaptchaFailure(message: "请管理员配置与验证码授权域名一致的 HTTPS 站点地址")
        }
        parts.scheme = "https"
        parts.path = "/"
        parts.query = nil
        parts.fragment = nil
        guard let url = parts.url else { throw CaptchaFailure(message: "验证码站点地址无效") }
        baseURL = url
    }
}

@MainActor
struct CaptchaView: View {
    let siteConfig: SiteConfig?
    let controller: CaptchaController

    var body: some View {
        if let site = siteConfig, site.isCaptcha {
            VStack(alignment: .leading, spacing: 8) {
                if let configuration = try? CaptchaConfiguration(site) {
                    CaptchaWebView(configuration: configuration, controller: controller, revision: controller.revision)
                        .frame(maxWidth: .infinity)
                        .frame(height: controller.height)
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                    HStack {
                        if controller.busy || !controller.ready {
                            ProgressView().controlSize(.small)
                        }
                        Text(controller.token != nil ? "安全验证已完成" : (site.captchaType == "recaptcha-v3" ? "提交时自动完成安全验证" : "请完成安全验证"))
                            .font(.caption).foregroundStyle(.secondary)
                        Spacer()
                        Button("重新加载") { controller.reload() }.font(.caption)
                    }
                    if let error = controller.error {
                        Text(error).font(.caption).foregroundStyle(.red)
                    }
                    if site.captchaType.hasPrefix("recaptcha") {
                        HStack(spacing: 8) {
                            Text("reCAPTCHA").foregroundStyle(.secondary)
                            Link("隐私政策", destination: URL(string: "https://policies.google.com/privacy")!)
                            Link("服务条款", destination: URL(string: "https://policies.google.com/terms")!)
                        }.font(.caption2)
                    }
                } else {
                    Label(configurationError(site), systemImage: "exclamationmark.shield")
                        .font(.footnote).foregroundStyle(.orange)
                }
            }
            .padding(12)
            .background(Color.white, in: RoundedRectangle(cornerRadius: 16))
        }
    }

    private func configurationError(_ site: SiteConfig) -> String {
        do { _ = try CaptchaConfiguration(site); return "" }
        catch { return error.localizedDescription }
    }
}

@MainActor
private struct CaptchaWebView: UIViewRepresentable {
    let configuration: CaptchaConfiguration
    let controller: CaptchaController
    let revision: Int

    func makeCoordinator() -> Coordinator { Coordinator(controller: controller) }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = true
        config.preferences.javaScriptCanOpenWindowsAutomatically = false
        config.websiteDataStore = .default()
        config.userContentController.add(context.coordinator, name: "sufeCaptcha")
        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        view.isOpaque = false
        view.backgroundColor = .clear
        view.scrollView.backgroundColor = .clear
        view.scrollView.isScrollEnabled = true
        view.allowsBackForwardNavigationGestures = false
        context.coordinator.load(view, configuration: configuration, revision: revision)
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) {
        if context.coordinator.configuration != configuration || context.coordinator.revision != revision {
            context.coordinator.load(view, configuration: configuration, revision: revision)
        }
    }

    static func dismantleUIView(_ view: WKWebView, coordinator: Coordinator) {
        view.stopLoading()
        view.configuration.userContentController.removeScriptMessageHandler(forName: "sufeCaptcha")
        view.navigationDelegate = nil
        coordinator.controller.detach()
    }

    @MainActor
    final class Coordinator: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
        let controller: CaptchaController
        var configuration: CaptchaConfiguration?
        var revision = -1
        private var nonce = ""
        private var loaded = false
        private let providerHosts = Set(["challenges.cloudflare.com", "www.recaptcha.net", "www.google.com", "recaptcha.google.com", "www.gstatic.com"])

        init(controller: CaptchaController) { self.controller = controller }

        func load(_ view: WKWebView, configuration: CaptchaConfiguration, revision: Int) {
            view.stopLoading()
            self.configuration = configuration
            self.revision = revision
            self.nonce = UUID().uuidString
            self.loaded = false
            controller.attach(view, provider: configuration.provider)
            view.loadHTMLString(html(configuration, nonce: nonce), baseURL: configuration.baseURL)
        }

        func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
            guard message.name == "sufeCaptcha", message.frameInfo.isMainFrame,
                  let configuration, let body = message.body as? [String: Any],
                  body["nonce"] as? String == nonce,
                  message.frameInfo.securityOrigin.protocol == "https",
                  message.frameInfo.securityOrigin.host.lowercased() == configuration.baseURL.host?.lowercased(),
                  let kind = body["kind"] as? String else { return }
            let expectedPort = configuration.baseURL.port ?? 443
            let actualPort = message.frameInfo.securityOrigin.port
            guard actualPort == expectedPort || (actualPort == 0 && expectedPort == 443) else { return }
            if kind == "height", let height = body["height"] as? Double {
                controller.height = CGFloat(min(max(height, 92), 620))
                return
            }
            controller.receive(kind, value: body["token"] as? String)
        }

        func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
            guard let target = navigationAction.targetFrame, let url = navigationAction.request.url,
                  let configuration else { decisionHandler(.cancel); return }
            if target.isMainFrame {
                let initial = !loaded && navigationAction.navigationType == .other && (url == configuration.baseURL || url.absoluteString == "about:blank")
                decisionHandler(initial ? .allow : .cancel)
                return
            }
            if ["about:blank", "about:srcdoc"].contains(url.absoluteString) {
                decisionHandler(.allow)
                return
            }
            let allowed = url.scheme?.lowercased() == "https" && providerHosts.contains(url.host?.lowercased() ?? "")
            decisionHandler(allowed ? .allow : .cancel)
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { loaded = true }
        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { navigationFailed(error) }
        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { navigationFailed(error) }
        func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { controller.fail("验证码页面已停止，请重新加载") }

        private func navigationFailed(_ error: Error) {
            guard (error as NSError).code != NSURLErrorCancelled else { return }
            controller.fail("无法加载验证码，请检查网络后重新加载")
        }

        private func html(_ configuration: CaptchaConfiguration, nonce: String) -> String {
            let data = try? JSONSerialization.data(withJSONObject: ["provider": configuration.provider, "siteKey": configuration.siteKey, "nonce": nonce])
            let settings = data.flatMap { String(data: $0, encoding: .utf8) }?.replacingOccurrences(of: "<", with: "\\u003c") ?? "{}"
            // Every HTML/script instruction is app-owned; the public settings
            // enter only as escaped JSON. No account content is rendered here.
            return """
            <!doctype html><html><head><meta name="viewport" content="width=device-width,initial-scale=1">
            <meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-\(nonce)' https://challenges.cloudflare.com https://www.recaptcha.net https://www.google.com https://www.gstatic.com; frame-src https://challenges.cloudflare.com https://www.recaptcha.net https://www.google.com https://recaptcha.google.com about:; connect-src https://challenges.cloudflare.com https://www.recaptcha.net https://www.google.com https://www.gstatic.com; img-src https://challenges.cloudflare.com https://www.gstatic.com data:; style-src 'unsafe-inline'; worker-src blob:; base-uri 'none'; form-action 'none'">
            <style>html,body{margin:0;padding:0;background:#fff;font:14px -apple-system,sans-serif}#widget{padding:6px 0;min-height:76px;display:flex;justify-content:center}</style></head>
            <body><div id="widget"></div><script nonce="\(nonce)">
            (() => {
              const config = \(settings); let widget = null; let sequence = 0;
              const send = (kind, extra = {}) => window.webkit.messageHandlers.sufeCaptcha.postMessage({nonce: config.nonce, kind, ...extra});
              const error = () => send('error');
              const success = token => send('token', {token});
              window.sufeReset = () => { sequence++; if(widget !== null) { if(config.provider === 'turnstile') turnstile.reset(widget); else if(config.provider === 'recaptcha') grecaptcha.reset(widget); } };
              window.sufeExecute = action => {
                if(config.provider !== 'recaptcha-v3' || !['login','register','send_email','reset_password'].includes(action)) return error();
                const current = ++sequence;
                grecaptcha.execute(config.siteKey, {action}).then(token => {if(current === sequence) success(token);}).catch(() => {if(current === sequence) error();});
              };
              window.sufeReady = () => {
                try {
                  const options = {sitekey:config.siteKey, callback:success, 'expired-callback':() => send('expired'), 'error-callback':error, theme:'light'};
                  if(config.provider === 'turnstile') { widget = turnstile.render('#widget', {...options, size:'flexible'}); send('ready'); }
                  else if(config.provider === 'recaptcha') { grecaptcha.ready(() => {widget = grecaptcha.render('widget', {...options, size:window.innerWidth < 310 ? 'compact' : 'normal'}); send('ready');}); }
                  else { grecaptcha.ready(() => send('ready')); }
                } catch (_) { error(); }
              };
              const script = document.createElement('script'); script.nonce = config.nonce; script.async = true; script.defer = true; script.onerror = error;
              script.src = config.provider === 'turnstile' ? 'https://challenges.cloudflare.com/turnstile/v0/api.js?onload=sufeReady&render=explicit' : 'https://www.recaptcha.net/recaptcha/api.js?onload=sufeReady&render=' + (config.provider === 'recaptcha-v3' ? encodeURIComponent(config.siteKey) : 'explicit');
              document.head.appendChild(script);
              let lastHeight = 0;
              const measure = () => {
                let height = config.provider === 'recaptcha' && window.innerWidth < 310 ? 160 : 92;
                document.querySelectorAll('iframe').forEach(frame => {
                  const rect = frame.getBoundingClientRect(); const style = getComputedStyle(frame);
                  if(style.visibility !== 'hidden' && rect.width > 0 && rect.height > 150) height = Math.max(height, rect.bottom + 12);
                });
                height = Math.min(620, Math.ceil(height));
                if(height !== lastHeight) { lastHeight = height; send('height', {height}); }
              };
              new MutationObserver(measure).observe(document.body, {childList:true, subtree:true, attributes:true, attributeFilter:['style','class']});
              window.addEventListener('resize', measure); measure();
            })();
            </script></body></html>
            """
        }
    }
}
