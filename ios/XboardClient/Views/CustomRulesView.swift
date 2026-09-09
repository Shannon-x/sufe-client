import SwiftUI

@MainActor
struct CustomRulesView: View {
    @Bindable var model: AppModel
    @State private var adding = false
    @State private var kind = "DOMAIN-SUFFIX"
    @State private var value = ""
    @State private var target = "PROXY"
    @State private var error: String?

    var body: some View {
        Group {
            if !model.clientFeatures.enabled("custom_rules") {
                ContentUnavailableView("自定义规则暂未开放", systemImage: "arrow.triangle.branch")
            } else {
                List {
                    Section {
                        Text("自定义规则优先于订阅规则。保存后，下次连接生效。")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    if model.customRules.isEmpty {
                        ContentUnavailableView("让常用网站，走你想走的路", systemImage: "arrow.triangle.branch", description: Text("添加域名或 IP 规则，选择代理、直连或拦截。"))
                    }
                    ForEach(model.customRules) { rule in
                        HStack {
                            VStack(alignment: .leading, spacing: 5) {
                                Text(rule.value).font(.subheadline).textSelection(.enabled)
                                Text("\(rule.kind) · \(targetName(rule.target))").font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Toggle("启用 \(rule.value)", isOn: Binding(get: { rule.enabled }, set: { enabled in
                                let changed = model.customRules.map { item in
                                    var next = item; if next.id == rule.id { next.enabled = enabled }; return next
                                }
                                save(changed)
                            })).labelsHidden().disabled(model.rulesBusy)
                        }
                    }.onDelete { offsets in
                        var changed = model.customRules; changed.remove(atOffsets: offsets); save(changed)
                    }
                }.listStyle(.insetGrouped)
            }
        }
        .navigationTitle("分流规则")
        .toolbar {
            if model.clientFeatures.enabled("custom_rules") {
                Button { value = ""; error = nil; adding = true } label: { Image(systemName: "plus") }
                    .accessibilityLabel("添加规则").disabled(model.rulesBusy || model.customRules.count >= 200)
            }
        }
        .task { await model.refreshCustomRules() }
        .sheet(isPresented: $adding) {
            NavigationStack {
                Form {
                    Picker("匹配类型", selection: $kind) {
                        Text("域名及子域名").tag("DOMAIN-SUFFIX")
                        Text("完整域名").tag("DOMAIN")
                        Text("域名关键词").tag("DOMAIN-KEYWORD")
                        Text("IPv4 网段").tag("IP-CIDR")
                        Text("IPv6 网段").tag("IP-CIDR6")
                    }
                    TextField("例如 example.com", text: $value).textInputAutocapitalization(.never).autocorrectionDisabled()
                    Picker("处理方式", selection: $target) {
                        Text("通过代理").tag("PROXY"); Text("直接连接").tag("DIRECT"); Text("拦截请求").tag("REJECT")
                    }
                    if let error { Text(error).foregroundStyle(.red).font(.caption) }
                    Button("保存规则") {
                        let input = value.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !input.isEmpty, !input.contains(","), !input.contains("\n") else {
                            error = "请输入有效的域名或网段，不要包含逗号或换行。"; return
                        }
                        let rule = MobileRule(id: UUID().uuidString, kind: kind, value: input, target: target, enabled: true)
                        Task {
                            do { try await model.saveCustomRules(model.customRules + [rule]); adding = false }
                            catch { self.error = model.friendly(error) }
                        }
                    }.disabled(model.rulesBusy || value.isEmpty)
                }.navigationTitle("添加规则")
                    .toolbar { Button("取消") { adding = false }.disabled(model.rulesBusy) }
            }.presentationDetents([.medium, .large])
        }
    }
    private func targetName(_ raw: String) -> String {
        ["PROXY": "代理", "DIRECT": "直连", "REJECT": "拦截"][raw] ?? raw
    }
    private func save(_ rules: [MobileRule]) {
        Task { do { try await model.saveCustomRules(rules) } catch { model.snackbar = model.friendly(error) } }
    }
}
