import Foundation
import SwiftUI

/// Format a Unix timestamp (seconds) as a localized "yyyy-MM-dd HH:mm" string.
/// Mirrors `formatDateTime` in the Android codebase.
func formatDateTime(_ timestamp: Int64) -> String {
    let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
    let f = DateFormatter()
    f.locale = Locale.current
    f.dateFormat = "yyyy-MM-dd HH:mm"
    return f.string(from: date)
}

/// Bytes → human (1.2 GB / 845 MB / etc.)
func formatBytes(_ bytes: UInt64) -> String {
    let f = ByteCountFormatter()
    f.allowedUnits = [.useAll]
    f.countStyle = .binary
    return f.string(fromByteCount: Int64(bytes))
}

/// Cents → "¥12.34" / localized currency. Falls back to the raw fraction
/// if the locale's currency is unknown.
func formatPriceCents(_ cents: Int64, currencyCode: String = "CNY") -> String {
    let f = NumberFormatter()
    f.numberStyle = .currency
    f.currencyCode = currencyCode
    f.locale = Locale.current
    return f.string(from: NSNumber(value: Double(cents) / 100.0)) ?? "\(Double(cents) / 100.0)"
}

extension View {
    /// Standard 16-pt page padding; applied at the top level of every screen.
    func screenPadding() -> some View {
        self.padding(.horizontal, 16).padding(.vertical, 8)
    }
}
