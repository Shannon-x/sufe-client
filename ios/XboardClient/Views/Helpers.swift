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

enum ProtonStyle {
    static let background = Color(red: 0.08, green: 0.07, blue: 0.12)
    static let panel = Color(red: 0.11, green: 0.09, blue: 0.16)
    static let panelBorder = Color.white.opacity(0.09)
    static let textMuted = Color(red: 0.75, green: 0.70, blue: 0.82)
    static let accent = Color(red: 0.49, green: 0.36, blue: 1.00)
    static let green = Color(red: 0.00, green: 0.82, blue: 0.61)
    static let danger = Color(red: 1.00, green: 0.25, blue: 0.34)
}

struct GeoPoint: Hashable {
    let label: String
    let country: String
    let flag: String
    let lat: Double
    let lon: Double
    let keys: [String]
}

struct GeoMapPin: Identifiable, Hashable {
    var id: String { "\(lat):\(lon)" }
    let label: String
    let country: String
    let flag: String
    let lat: Double
    let lon: Double
    let x: Double
    let y: Double
    let count: Int
    let active: Bool
}

let geoAliases: [GeoPoint] = [
    .init(label: "Taipei", country: "台湾", flag: "🇹🇼", lat: 25.033, lon: 121.565, keys: ["台北", "taipei"]),
    .init(label: "Taiwan", country: "台湾", flag: "🇹🇼", lat: 23.697, lon: 120.961, keys: ["台湾", "台灣", "taiwan", " tw"]),
    .init(label: "Tokyo", country: "日本", flag: "🇯🇵", lat: 35.676, lon: 139.650, keys: ["东京", "東京", "tokyo"]),
    .init(label: "Osaka", country: "日本", flag: "🇯🇵", lat: 34.694, lon: 135.502, keys: ["大阪", "osaka"]),
    .init(label: "Japan", country: "日本", flag: "🇯🇵", lat: 36.204, lon: 138.253, keys: ["日本", "japan", " jp"]),
    .init(label: "Hong Kong", country: "香港", flag: "🇭🇰", lat: 22.319, lon: 114.169, keys: ["香港", "hong kong", "hongkong", " hk"]),
    .init(label: "Singapore", country: "新加坡", flag: "🇸🇬", lat: 1.352, lon: 103.820, keys: ["新加坡", "singapore", " sg"]),
    .init(label: "Seoul", country: "韩国", flag: "🇰🇷", lat: 37.566, lon: 126.978, keys: ["首尔", "首爾", "seoul"]),
    .init(label: "Korea", country: "韩国", flag: "🇰🇷", lat: 36.500, lon: 127.800, keys: ["韩国", "韓國", "korea", " kr"]),
    .init(label: "Los Angeles", country: "美国", flag: "🇺🇸", lat: 34.052, lon: -118.244, keys: ["洛杉矶", "洛杉磯", "los angeles", "la-"]),
    .init(label: "San Jose", country: "美国", flag: "🇺🇸", lat: 37.338, lon: -121.886, keys: ["圣何塞", "聖何塞", "san jose", "sanjose"]),
    .init(label: "New York", country: "美国", flag: "🇺🇸", lat: 40.713, lon: -74.006, keys: ["纽约", "紐約", "new york"]),
    .init(label: "United States", country: "美国", flag: "🇺🇸", lat: 39.828, lon: -98.579, keys: ["美国", "美國", "united states", "usa", " us"]),
    .init(label: "London", country: "英国", flag: "🇬🇧", lat: 51.507, lon: -0.128, keys: ["伦敦", "倫敦", "london"]),
    .init(label: "United Kingdom", country: "英国", flag: "🇬🇧", lat: 54.000, lon: -2.000, keys: ["英国", "英國", "united kingdom", " uk"]),
    .init(label: "Frankfurt", country: "德国", flag: "🇩🇪", lat: 50.110, lon: 8.682, keys: ["法兰克福", "法蘭克福", "frankfurt"]),
    .init(label: "Germany", country: "德国", flag: "🇩🇪", lat: 51.165, lon: 10.452, keys: ["德国", "德國", "germany", " de"]),
    .init(label: "Paris", country: "法国", flag: "🇫🇷", lat: 48.857, lon: 2.352, keys: ["巴黎", "paris", "法国", "法國", "france"]),
    .init(label: "Amsterdam", country: "荷兰", flag: "🇳🇱", lat: 52.367, lon: 4.904, keys: ["阿姆斯特丹", "amsterdam", "荷兰", "荷蘭", "netherlands", " nl"]),
    .init(label: "Sydney", country: "澳大利亚", flag: "🇦🇺", lat: -33.869, lon: 151.209, keys: ["悉尼", "sydney"]),
    .init(label: "Australia", country: "澳大利亚", flag: "🇦🇺", lat: -25.274, lon: 133.775, keys: ["澳大利亚", "澳洲", "australia", " au"]),
    .init(label: "Toronto", country: "加拿大", flag: "🇨🇦", lat: 43.653, lon: -79.383, keys: ["多伦多", "多倫多", "toronto"]),
    .init(label: "Canada", country: "加拿大", flag: "🇨🇦", lat: 56.130, lon: -106.347, keys: ["加拿大", "canada", " ca"]),
    .init(label: "Bangkok", country: "泰国", flag: "🇹🇭", lat: 13.756, lon: 100.501, keys: ["曼谷", "bangkok", "泰国", "泰國", "thailand"]),
    .init(label: "Ho Chi Minh City", country: "越南", flag: "🇻🇳", lat: 10.823, lon: 106.630, keys: ["胡志明", "越南", "vietnam", " vn"]),
    .init(label: "Manila", country: "菲律宾", flag: "🇵🇭", lat: 14.599, lon: 120.984, keys: ["马尼拉", "馬尼拉", "菲律宾", "菲律賓", "philippines"]),
    .init(label: "Kuala Lumpur", country: "马来西亚", flag: "🇲🇾", lat: 3.139, lon: 101.687, keys: ["吉隆坡", "马来", "馬來", "malaysia"]),
    .init(label: "Jakarta", country: "印尼", flag: "🇮🇩", lat: -6.208, lon: 106.846, keys: ["雅加达", "雅加達", "印尼", "indonesia"]),
    .init(label: "Mumbai", country: "印度", flag: "🇮🇳", lat: 19.076, lon: 72.878, keys: ["孟买", "孟買", "mumbai"]),
    .init(label: "India", country: "印度", flag: "🇮🇳", lat: 20.594, lon: 78.963, keys: ["印度", "india"]),
    .init(label: "Dubai", country: "阿联酋", flag: "🇦🇪", lat: 25.205, lon: 55.271, keys: ["迪拜", "dubai", "阿联酋", "阿聯酋", "uae"]),
    .init(label: "Istanbul", country: "土耳其", flag: "🇹🇷", lat: 41.008, lon: 28.978, keys: ["伊斯坦布尔", "土耳其", "turkey", "istanbul"]),
    .init(label: "Moscow", country: "俄罗斯", flag: "🇷🇺", lat: 55.756, lon: 37.617, keys: ["莫斯科", "俄罗斯", "俄羅斯", "russia"]),
    .init(label: "Shanghai", country: "中国", flag: "🇨🇳", lat: 31.231, lon: 121.474, keys: ["上海", "shanghai"]),
    .init(label: "Beijing", country: "中国", flag: "🇨🇳", lat: 39.904, lon: 116.407, keys: ["北京", "beijing", "中国", "中國", "china"]),
    .init(label: "Macau", country: "澳门", flag: "🇲🇴", lat: 22.199, lon: 113.544, keys: ["澳门", "澳門", "macau", "macao"]),
]

func locateNode(_ name: String) -> GeoPoint? {
    let separators = CharacterSet(charactersIn: "|｜_-·•/()[]{}")
    let normalized = " " + name.lowercased()
        .components(separatedBy: separators)
        .joined(separator: " ")
        .split(whereSeparator: \.isWhitespace)
        .joined(separator: " ") + " "
    return geoAliases.first { point in
        point.keys.contains { normalized.contains($0.lowercased()) }
    }
}

func collectMapPins(groups: [ProxyGroupSnapshot], activeNode: String?) -> [GeoMapPin] {
    var counts: [String: (GeoPoint, Int)] = [:]
    var names: [String] = []
    if let activeNode { names.append(activeNode) }
    names.append(contentsOf: groups.flatMap(\.all))
    for name in names {
        guard let point = locateNode(name) else { continue }
        let key = "\(point.lat):\(point.lon)"
        let old = counts[key]?.1 ?? 0
        counts[key] = (point, old + 1)
    }
    return counts.values.map { item in
        let point = item.0
        let projected = projectGeo(lat: point.lat, lon: point.lon)
        let active = activeNode.flatMap { locateNode($0) }?.label == point.label
        return GeoMapPin(
            label: point.label,
            country: point.country,
            flag: point.flag,
            lat: point.lat,
            lon: point.lon,
            x: projected.x,
            y: projected.y,
            count: item.1,
            active: active
        )
    }
    .sorted { lhs, rhs in
        if lhs.active != rhs.active { return lhs.active }
        return lhs.count > rhs.count
    }
    .prefix(28)
    .map { $0 }
}

func projectGeo(lat: Double, lon: Double) -> (x: Double, y: Double) {
    let x = min(98, max(2, ((lon + 180) / 360) * 100))
    let clampedLat = min(85, max(-85, lat))
    let latRad = clampedLat * Double.pi / 180
    let merc = log(tan(Double.pi / 4 + latRad / 2))
    let y = min(94, max(6, ((1 - merc / Double.pi) / 2) * 100))
    return (x, y)
}
