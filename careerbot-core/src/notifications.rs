//! Notification dispatch — the seam between the daemon's "new jobs
//! happened" event and whatever surface (OS notification, email,
//! webhook…) the user wants to be reached on.
//!
//! Phase 5 ships `OsChannel` only; `EmailChannel` and `WebhookChannel`
//! are listed for later in PLAN.md §9. The trait keeps `OsChannel`'s
//! OS side-effect mockable: scheduler tests use a `MockChannel` that
//! records sends to a `Vec` instead of touching the user's
//! Notification Center.

use async_trait::async_trait;

/// One outbound notification.
#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub body: String,
    /// Click target — populated only when there's a single canonical
    /// URL to open (currently: single-job notifications). Multi-job
    /// batched notifications leave this `None` until the local HTML
    /// view at `/jobs` lands.
    pub click_url: Option<String>,
}

#[derive(Debug)]
pub enum NotificationError {
    Backend(String),
}

impl std::fmt::Display for NotificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backend(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for NotificationError {}

#[async_trait]
pub trait NotificationChannel: Send + Sync {
    async fn send(&self, notification: Notification) -> Result<(), NotificationError>;
    /// Persisted to `notifications.channel` — keep stable; the schema
    /// already uses these string discriminants.
    fn name(&self) -> &'static str;
}

/// OS-native notification channel via `notify-rust`. Calls the
/// platform notification API directly (`NSUserNotification` on macOS,
/// libnotify/dbus on Linux, Toast on Windows).
pub struct OsChannel;

impl OsChannel {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OsChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationChannel for OsChannel {
    async fn send(&self, notification: Notification) -> Result<(), NotificationError> {
        let title = notification.title.clone();
        let body = notification.body.clone();
        // `notify-rust` is synchronous; offload to a blocking task so a
        // slow dbus round-trip on Linux doesn't stall the scheduler.
        tokio::task::spawn_blocking(move || {
            notify_rust::Notification::new()
                .summary(&title)
                .body(&body)
                .show()
                .map(|_| ())
                .map_err(|e| NotificationError::Backend(e.to_string()))
        })
        .await
        .map_err(|e| NotificationError::Backend(format!("spawn: {e}")))?
    }

    fn name(&self) -> &'static str {
        "os"
    }
}

/// In-memory channel for tests. Lives under `#[cfg(test)]` so it
/// doesn't ship in release builds, but is `pub(crate)` so the
/// scheduler tests can use it.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    pub struct MockChannel {
        pub sent: Arc<Mutex<Vec<Notification>>>,
    }

    impl MockChannel {
        pub fn new() -> (Self, Arc<Mutex<Vec<Notification>>>) {
            let sent = Arc::new(Mutex::new(Vec::new()));
            (Self { sent: sent.clone() }, sent)
        }
    }

    #[async_trait]
    impl NotificationChannel for MockChannel {
        async fn send(&self, n: Notification) -> Result<(), NotificationError> {
            self.sent.lock().await.push(n);
            Ok(())
        }
        fn name(&self) -> &'static str {
            "mock"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::MockChannel;

    #[tokio::test]
    async fn mock_channel_records_sent_notifications() {
        let (channel, recorded) = MockChannel::new();
        channel
            .send(Notification {
                title: "Microsoft".into(),
                body: "Senior SWE".into(),
                click_url: Some("https://x".into()),
            })
            .await
            .unwrap();
        let messages = recorded.lock().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].title, "Microsoft");
    }
}
