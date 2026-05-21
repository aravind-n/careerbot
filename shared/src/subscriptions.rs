use chrono::{DateTime, Utc};
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents user subscriptions
///
/// This struct contains info about a user's subscriptions to various
/// company job postings
#[derive(Debug, Default, Clone, Builder, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
#[builder(setter(into, strip_option), default)]
pub struct Subscriptions {
    #[serde(default)]
    #[builder(default = Uuid::new_v4())]
    pub(crate) id: Uuid,

    pub(crate) user_id: Uuid,

    pub(crate) company_tag: String,

    pub(crate) query_string: String,

    pub(crate) exclude_string: String,

    #[serde(default = "Utc::now")]
    #[builder(default = "Utc::now()")]
    pub(crate) created_at: DateTime<Utc>,
}
