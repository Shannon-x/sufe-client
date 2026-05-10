import SwiftUI

struct NoticesView: View {
    @Bindable var model: AppModel

    var body: some View {
        Group {
            if let notices = model.notices {
                if notices.isEmpty {
                    ContentUnavailableView(
                        String(localized: "notices.empty"),
                        systemImage: "bell"
                    )
                } else {
                    List(notices, id: \.id) { notice in
                        NoticeCard(notice: notice)
                            .listRowSeparator(.hidden)
                    }
                    .listStyle(.plain)
                }
            } else {
                ProgressView()
            }
        }
        .navigationTitle(String(localized: "notices.title"))
        .refreshable { await model.refreshNotices() }
        .task { await model.refreshNotices() }
    }
}

private struct NoticeCard: View {
    let notice: Notice

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(notice.title.isEmpty
                 ? String(localized: "notices.untitled")
                 : notice.title)
                .font(.subheadline.weight(.semibold))

            if !notice.tags.isEmpty {
                HStack(spacing: 4) {
                    ForEach(notice.tags, id: \.self) { tag in
                        Text(tag)
                            .font(.caption)
                            .padding(.horizontal, 8).padding(.vertical, 2)
                            .background(Color.accentColor.opacity(0.12), in: Capsule())
                            .foregroundStyle(Color.accentColor)
                    }
                }
            }

            if !notice.content.isEmpty {
                Text(notice.content)
                    .font(.callout)
                    .foregroundStyle(.primary)
            }

            if let created = notice.createdAt {
                Text(formatDateTime(created))
                    .font(.caption2).foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 16))
        .padding(.vertical, 4)
    }
}
