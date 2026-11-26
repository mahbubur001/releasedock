//! Port of `Services/AppServices.swift`: the notification and self-update services whose
//! implementations live in the app (Tauri), not in the core.

use async_trait::async_trait;
use rd_integrations::ReleaseChannel;

/// User-facing notifications. The concrete implementation (the OS notification centre)
/// lives in the app because it needs the app's identity.
#[async_trait]
pub trait NotificationServicing: Send + Sync {
    async fn request_authorization(&self) -> bool;
    async fn notify(&self, title: &str, body: &str, identifier: &str);
}

/// Notifications that go nowhere (tests, headless use).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopNotificationService;

impl NoopNotificationService {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl NotificationServicing for NoopNotificationService {
    async fn request_authorization(&self) -> bool {
        false
    }

    async fn notify(&self, _title: &str, _body: &str, _identifier: &str) {}
}

/// Application self-update. The implementation lives in the app; it only installs updates
/// whose signature verifies. Swift's `@MainActor` class protocol with settable properties
/// becomes getters and setters on `&mut self`.
pub trait UpdateServicing {
    fn can_check_for_updates(&self) -> bool;
    fn automatically_checks_for_updates(&self) -> bool;
    fn set_automatically_checks_for_updates(&mut self, value: bool);
    fn channel(&self) -> ReleaseChannel;
    fn set_channel(&mut self, channel: ReleaseChannel);
    fn check_for_updates(&mut self);
}
