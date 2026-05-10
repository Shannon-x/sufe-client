import Foundation
import Security

/// Bridges UniFFI's `SecureStore` callback interface onto iOS Keychain.
/// Items are stored under a shared Keychain access group so the
/// `PacketTunnel` extension can read the same bearer the app wrote.
///
/// Only the values fit through the FFI — keys the host writes are owned
/// by the Rust side (`storage::keys`).
final class KeychainSecureStore: SecureStore {
    private let accessGroup: String?
    private let service = "com.xboard.client"

    init(accessGroup: String? = "$(AppIdentifierPrefix)com.xboard.client") {
        self.accessGroup = accessGroup
    }

    private func baseQuery(_ key: String) -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            // iOS unlocks Keychain after the first user PIN entry post-boot;
            // NE extensions need this accessibility to read the bearer
            // when the device is locked but the user has unlocked at least once.
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlock,
        ]
        if let group = accessGroup {
            q[kSecAttrAccessGroup as String] = group
        }
        return q
    }

    func put(key: String, value: String) throws {
        guard let data = value.data(using: .utf8) else {
            throw StorageError.Backend(message: "value not UTF-8")
        }
        var query = baseQuery(key)
        // Try update first, then add — avoids a "duplicate item" race when
        // the same key is written twice in quick succession.
        let attrs: [String: Any] = [kSecValueData as String: data]
        let status = SecItemUpdate(query as CFDictionary, attrs as CFDictionary)
        switch status {
        case errSecSuccess:
            return
        case errSecItemNotFound:
            query[kSecValueData as String] = data
            let addStatus = SecItemAdd(query as CFDictionary, nil)
            if addStatus != errSecSuccess {
                throw StorageError.Backend(message: "SecItemAdd \(addStatus)")
            }
        default:
            throw StorageError.Backend(message: "SecItemUpdate \(status)")
        }
    }

    func get(key: String) throws -> String? {
        var query = baseQuery(key)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        switch status {
        case errSecSuccess:
            guard let data = item as? Data, let s = String(data: data, encoding: .utf8) else {
                throw StorageError.Backend(message: "Keychain returned non-UTF8 blob")
            }
            return s
        case errSecItemNotFound:
            return nil
        default:
            throw StorageError.Backend(message: "SecItemCopyMatching \(status)")
        }
    }

    func delete(key: String) throws {
        let query = baseQuery(key)
        let status = SecItemDelete(query as CFDictionary)
        if status != errSecSuccess && status != errSecItemNotFound {
            throw StorageError.Backend(message: "SecItemDelete \(status)")
        }
    }
}
