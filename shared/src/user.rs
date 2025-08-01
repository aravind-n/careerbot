use std::fmt::Display;

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents a careerbot user
///
/// This struct contains information about careerbot users
/// including their ID, email, and in the future, other contact information
#[derive(Debug, Default, Clone, Builder, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
#[builder(setter(into, strip_option), default)]
pub struct User {
    #[serde(default)]
    #[builder(default = Uuid::new_v4())]
    pub(crate) id: Uuid,

    pub(crate) email: String,

    #[serde(default = "Utc::now")]
    #[builder(default = "Utc::now()")]
    pub(crate) created_at: DateTime<Utc>,
}

impl Display for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Email: {}", self.email,)
    }
}
