import Foundation
import Security

public enum KeychainError: Error, LocalizedError, Equatable, Sendable {
    case unexpectedStatus(OSStatus)
    case invalidData

    public var errorDescription: String? {
        switch self {
        case .unexpectedStatus(let status):
            let message = SecCopyErrorMessageString(status, nil) as String? ?? "OSStatus \(status)"
            return "Keychain error: \(message)"
        case .invalidData: return "Keychain item has invalid data"
        }
    }
}

/// A credential identity. `service` groups credentials by kind; `account` is the
/// user-visible identifier (an SVN username or a GitHub token label).
public struct KeychainItem: Hashable, Sendable {
    public var service: String
    public var account: String

    public init(service: String, account: String) {
        self.service = service
        self.account = account
    }

    /// SVN password for a repository host (e.g. `plugins.svn.wordpress.org`) and username.
    public static func svn(host: String, username: String) -> KeychainItem {
        KeychainItem(service: "com.releasedock.svn.\(host)", account: username)
    }

    /// GitHub token used for private ReleaseDock update checks.
    public static let githubToken = KeychainItem(service: "com.releasedock.github", account: "update-check-token")
}

public protocol KeychainServicing: Sendable {
    func password(for item: KeychainItem) throws -> String?
    func setPassword(_ password: String, for item: KeychainItem) throws
    func deletePassword(for item: KeychainItem) throws
}

/// Generic-password storage in the user's login keychain.
///
/// The data-protection keychain (`kSecUseDataProtectionKeychain`) requires a
/// keychain-access-group entitlement backed by a provisioning profile, which a
/// Developer ID app without one does not have (errSecMissingEntitlement). ReleaseDock
/// therefore uses the file-based login keychain, whose default access control trusts
/// only the app that created the item.
public struct KeychainService: KeychainServicing {
    public init() {}

    private func baseQuery(_ item: KeychainItem) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: item.service,
            kSecAttrAccount as String: item.account,
        ]
    }

    public func password(for item: KeychainItem) throws -> String? {
        var query = baseQuery(item)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        switch status {
        case errSecSuccess:
            guard let data = result as? Data, let string = String(data: data, encoding: .utf8) else {
                throw KeychainError.invalidData
            }
            return string
        case errSecItemNotFound:
            return nil
        default:
            throw KeychainError.unexpectedStatus(status)
        }
    }

    public func setPassword(_ password: String, for item: KeychainItem) throws {
        let data = Data(password.utf8)
        let update: [String: Any] = [kSecValueData as String: data]
        let status = SecItemUpdate(baseQuery(item) as CFDictionary, update as CFDictionary)
        if status == errSecItemNotFound {
            var add = baseQuery(item)
            add[kSecValueData as String] = data
            add[kSecAttrLabel as String] = "ReleaseDock: \(item.service)"
            add[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlocked
            let addStatus = SecItemAdd(add as CFDictionary, nil)
            guard addStatus == errSecSuccess else { throw KeychainError.unexpectedStatus(addStatus) }
        } else if status != errSecSuccess {
            throw KeychainError.unexpectedStatus(status)
        }
        Log.keychain.info("Stored credential for service \(item.service, privacy: .public)")
    }

    public func deletePassword(for item: KeychainItem) throws {
        let status = SecItemDelete(baseQuery(item) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw KeychainError.unexpectedStatus(status) }
    }
}

/// In-memory keychain for tests and previews.
public final class InMemoryKeychain: KeychainServicing, @unchecked Sendable {
    private let lock = NSLock()
    private var items: [KeychainItem: String] = [:]

    public init(_ items: [KeychainItem: String] = [:]) {
        self.items = items
    }

    public func password(for item: KeychainItem) throws -> String? { lock.withLock { items[item] } }
    public func setPassword(_ password: String, for item: KeychainItem) throws { lock.withLock { items[item] = password } }
    public func deletePassword(for item: KeychainItem) throws { _ = lock.withLock { items.removeValue(forKey: item) } }
}
