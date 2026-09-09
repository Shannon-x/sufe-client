import SwiftUI

@MainActor
struct SupportView: View {
    @Bindable var model: AppModel
    @State private var draft = ""
    @State private var sendError: String?
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        VStack(spacing: 0) {
            if model.clientFeatures.enabled("chatwoot") {
                ScrollViewReader { reader in
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 14) {
                            if model.chatMessages.isEmpty {
                                ContentUnavailableView("你好，有什么可以帮你？", systemImage: "bubble.left.and.bubble.right", description: Text("关于订阅、连接或购买的问题，都可以在这里告诉我们。"))
                            }
                            ForEach(model.chatMessages) { message in
                                HStack {
                                    if !message.incoming { Spacer(minLength: 40) }
                                    VStack(alignment: .leading, spacing: 8) {
                                        Text(message.incoming ? "客户支持" : "你").font(.caption2).foregroundStyle(.secondary)
                                        if let text = message.content, !text.isEmpty { Text(text).font(.body).textSelection(.enabled) }
                                        ForEach(message.attachments ?? []) { attachment in
                                            if let url = attachment.safeURL {
                                                Link(destination: url) {
                                                    Label(attachment.fallbackTitle ?? "查看附件", systemImage: "paperclip")
                                                }.font(.callout)
                                            } else { Text("附件暂时无法查看").font(.caption).foregroundStyle(.secondary) }
                                        }
                                    }
                                    .padding(14)
                                    .background(message.incoming ? ProtonStyle.panel : ProtonStyle.accent.opacity(0.10), in: RoundedRectangle(cornerRadius: 16))
                                    if message.incoming { Spacer(minLength: 40) }
                                }.id(message.id)
                            }
                        }.padding(18)
                    }
                    .onChange(of: model.chatMessages.last?.id) { _, id in
                        if let id { withAnimation { reader.scrollTo(id, anchor: .bottom) } }
                    }
                }
                if let error = sendError ?? model.chatError {
                    HStack {
                        Text(error).font(.caption).foregroundStyle(.red)
                        Spacer()
                        Button("重试") { Task { await model.refreshChat() } }.font(.caption)
                    }.padding(.horizontal, 18).padding(.vertical, 8)
                }
                HStack(alignment: .bottom, spacing: 10) {
                    TextField("输入消息…", text: $draft, axis: .vertical).lineLimit(1...5)
                        .padding(12).background(ProtonStyle.background, in: RoundedRectangle(cornerRadius: 12))
                        .disabled(model.chatSending)
                    Button { send() } label: {
                        Image(systemName: model.chatSending ? "ellipsis" : "arrow.up")
                            .font(.headline).foregroundStyle(.white).padding(13)
                            .background(ProtonStyle.accent, in: Circle())
                    }.accessibilityLabel("发送消息")
                        .disabled(model.chatSending || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }.padding(14).background(ProtonStyle.panel)
            } else {
                ContentUnavailableView("在线客服暂未开放", systemImage: "bubble.left.and.bubble.right", description: Text("有问题可以通过工单联系支持团队。"))
            }
        }
        .background(ProtonStyle.background)
        .navigationTitle("客户支持")
        .toolbar {
            if model.clientFeatures.enabled("tickets") {
                NavigationLink { TicketsView(model: model) } label: { Label("工单", systemImage: "envelope") }
            }
        }
        .task { model.chatViewing = scenePhase == .active; await model.refreshChat(); model.markChatRead() }
        .onDisappear { model.chatViewing = false }
        .onChange(of: scenePhase) { _, phase in model.chatViewing = phase == .active; if model.chatViewing { model.markChatRead() } }
    }

    private func send() {
        let content = draft
        sendError = nil
        Task {
            do { try await model.sendChat(content); draft = "" }
            catch { sendError = model.friendly(error) }
        }
    }
}
