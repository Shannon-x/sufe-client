import Foundation

struct MobileClientConfig: Decodable {
    var brandName = "Sufe"
    var features: [String: Bool] = [:]
    var transportMode: String?
    var configSource: String?
    var chatwootBaseUrl: String?
    var chatwootInboxIdentifier: String?

    func enabled(_ name: String) -> Bool { features[name] == true }
}

enum JSONValue: Codable {
    case number(Double), string(String), bool(Bool), object([String: JSONValue]), array([JSONValue]), null

    init(from decoder: Decoder) throws {
        let box = try decoder.singleValueContainer()
        if box.decodeNil() { self = .null }
        else if let v = try? box.decode(Bool.self) { self = .bool(v) }
        else if let v = try? box.decode(Double.self) { self = .number(v) }
        else if let v = try? box.decode(String.self) { self = .string(v) }
        else if let v = try? box.decode([String: JSONValue].self) { self = .object(v) }
        else { self = .array(try box.decode([JSONValue].self)) }
    }

    func encode(to encoder: Encoder) throws {
        var box = encoder.singleValueContainer()
        switch self {
        case let .number(v): try box.encode(v)
        case let .string(v): try box.encode(v)
        case let .bool(v): try box.encode(v)
        case let .object(v): try box.encode(v)
        case let .array(v): try box.encode(v)
        case .null: try box.encodeNil()
        }
    }

    subscript(key: String) -> JSONValue {
        if case let .object(v) = self { return v[key] ?? .null }
        return .null
    }
    var number: Double? {
        if case let .number(v) = self { return v }
        if case let .string(v) = self { return Double(v) }
        return nil
    }
    var text: String {
        switch self {
        case let .string(v): return v
        case let .number(v): return v.formatted()
        case let .bool(v): return v ? "是" : "否"
        case .null: return ""
        default: return "包含多项权益"
        }
    }
}

func decodeCore<T: Decodable>(_ type: T.Type, _ json: String) throws -> T {
    let decoder = JSONDecoder()
    decoder.keyDecodingStrategy = .convertFromSnakeCase
    return try decoder.decode(type, from: Data(json.utf8))
}

struct GiftPreview: Decodable {
    let codeInfo: JSONValue
    let rewardPreview: [String: JSONValue]
    let canRedeem: Bool
    let reason: String?
    var mystery: Bool { codeInfo["template"]["type"].number == 3 }
    var name: String { codeInfo["template"]["name"].text }
    var rewards: [String] { rewardPreview.keys.filter { !["random_rewards", "weight"].contains($0) }.sorted() }
}
struct GiftReceipt: Decodable {
    let message: String
    let templateName: String
    let rewards: [String: JSONValue]
}
struct GiftHistory: Decodable {
    struct Entry: Decodable, Identifiable {
        let id: Int64
        let templateName: String
        let createdAt: JSONValue
        var date: String {
            if let value = createdAt.number, let seconds = Int64(exactly: value) { return formatDateTime(seconds) }
            return createdAt.text
        }
    }
    let data: [Entry]
}
struct InviteSummary: Decodable {
    struct Code: Decodable, Identifiable { let id: Int64; let code: String; let status: Int32 }
    let codes: [Code]
    let stat: [Double]
    func value(_ index: Int) -> Double { stat.indices.contains(index) ? stat[index] : 0 }
}
struct MobileRule: Codable, Identifiable, Equatable {
    var id: String
    var kind: String
    var value: String
    var target: String
    var enabled: Bool
}
struct ServerOrder: Decodable {
    let tradeNo: String
    let totalAmount: Int64
    let handlingAmount: Int64?
    let balanceAmount: Int64?
    let discountAmount: Int64?
    let paymentId: Int64?
    let status: Int32
}

struct ChatMessage: Decodable, Identifiable {
    struct Attachment: Decodable, Identifiable {
        let id: Int64
        let dataUrl: String?
        let fileType: String?
        let fallbackTitle: String?
        var safeURL: URL? {
            guard let raw = dataUrl, let url = URL(string: raw), url.scheme == "https" else { return nil }
            return url
        }
    }
    let id: Int64
    let content: String?
    let messageType: Int
    let createdAt: JSONValue
    let `private`: Bool?
    let attachments: [Attachment]?
    var incoming: Bool { messageType == 1 }
}
struct ChatConversation: Decodable, Identifiable {
    let id: Int64
    let status: String
}
struct ChatRestore: Decodable { let conversations: [ChatConversation] }

func rewardLabel(_ key: String) -> String {
    ["balance": "账户余额", "transfer_enable": "流量额度", "expire_days": "延长有效期",
     "reset_package": "重置已用流量", "device_limit": "增加设备数量", "plan_id": "订阅套餐",
     "invite_reward_rate": "邀请奖励比例"][key] ?? key
}
func rewardText(_ key: String, _ value: JSONValue, preview: GiftPreview? = nil) -> String {
    if let number = value.number, number.isFinite {
        switch key {
        case "balance": return Int64(exactly: number).map { formatPriceCents($0) } ?? "金额异常"
        case "transfer_enable": return UInt64(exactly: number).map(formatBytes) ?? "额度异常"
        case "expire_days": return Int(exactly: number).map { "\($0) 天" } ?? "天数异常"
        case "device_limit": return Int(exactly: number).map { "\($0) 台" } ?? "数量异常"
        case "invite_reward_rate": return Int(exactly: (number * 100).rounded()).map { "\($0)%" } ?? "比例异常"
        case "reset_package": return number == 0 ? "否" : "是"
        case "plan_id":
            let name = preview?.codeInfo["plan_info"]["name"].text ?? ""
            return name.isEmpty ? (Int64(exactly: number).map { "套餐 \($0)" } ?? "套餐信息异常") : name
        default: break
        }
    }
    return value.text
}
