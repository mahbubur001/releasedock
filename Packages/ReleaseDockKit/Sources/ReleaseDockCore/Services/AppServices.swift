import Foundation

/// User-facing notifications. The concrete implementation (UserNotifications) lives in
/// the app target because `UNUserNotificationCenter` requires an app bundle.
public protocol NotificationServicing: Sendable {
    func requestAuthorization() async -> Bool
    func notify(title: String, body: String, identifier: String) async
}

public struct NoopNotificationService: NotificationServicing {
    public init() {}
    public func requestAuthorization() async -> Bool { false }
    public func notify(title: String, body: String, identifier: String) async {}
}

/// Application self-update. The Sparkle-backed implementation lives in the app target;
/// it only installs updates whose EdDSA signature (and Developer ID signature) verify.
@MainActor
public protocol UpdateServicing: AnyObject {
    var canCheckForUpdates: Bool { get }
    var automaticallyChecksForUpdates: Bool { get set }
    var channel: ReleaseChannel { get set }
    func checkForUpdates()
}
