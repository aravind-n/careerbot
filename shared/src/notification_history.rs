use chrono::{DateTime, Utc};
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents a historical entry for a sent notification
///
/// This struct contains information about a particular sent notification
/// including the recepient user, job posting, and the sent timestamp
#[derive(Debug, Default, Clone, Builder, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
#[builder(setter(into, strip_option), default)]
pub struct NotificationHistory {
    #[serde(default)]
    #[builder(default = Uuid::new_v4())]
    pub(crate) id: Uuid,

    pub(crate) user_id: Uuid,

    pub(crate) job_id: Uuid,

    pub(crate) sent_at: DateTime<Utc>,

    #[serde(default = "Utc::now")]
    #[builder(default = "Utc::now()")]
    pub(crate) created_at: DateTime<Utc>,
}
