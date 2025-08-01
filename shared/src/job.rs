use std::fmt::Display;

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents a job listing with metadata collected from an external job portal.
///
/// This struct contains essential job information such as title, description,
/// employment type, and source-specific identifiers. It is designed to be built
/// using the `JobBuilder` and can be persisted via SQLx.
#[derive(Debug, Default, Clone, Builder, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
#[builder(setter(into, strip_option), default)]
pub struct Job {
    /// Unique identifier for the job record in the local system.
    ///
    /// Automatically generated using UUID v4.
    #[serde(default)]
    #[builder(default = Uuid::new_v4())]
    pub(crate) id: Uuid,

    /// Tag representing the company associated with the job.
    ///
    /// This is typically a short identifier like "MSFT" or "GOOG".
    /// TODO: Consider converting this to an enum for better type safety.
    pub company_tag: String,

    /// Unique identifier of the job on the external job portal.
    ///
    /// Used to detect duplicates and synchronize updates from the source.
    pub(crate) job_portal_id: String,

    /// Title or role name of the job listing.
    pub(crate) title: String,

    /// Optional detailed description of the job role.
    pub(crate) description: Option<String>,

    /// Optional employment type such as "Full-time", "Part-time", or "Contract".
    ///
    /// TODO: Consider converting this to an enum for defined variants.
    pub(crate) employment_type: Option<String>,

    /// Optional list of job location(s) associated with the listing.
    pub(crate) location: Option<Vec<String>>,

    /// Optional additional attributes not covered by the main fields.
    ///
    /// This can include department, work site type, job level, etc.
    pub(crate) other_data: Option<Vec<String>>,

    /// Optional posting date of the job, in UTC.
    ///
    /// Typically sourced from the external API.
    pub(crate) post_date: Option<DateTime<Utc>>,

    /// Timestamp of when the job record was created in the system.
    ///
    /// Defaults to the current UTC time.
    #[serde(default = "Utc::now")]
    #[builder(default = "Utc::now()")]
    pub(crate) created_at: DateTime<Utc>,
}

impl Display for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Company: {}, Job Portal ID: {}, Title: {}, Post Date: {}",
            self.company_tag,
            self.job_portal_id,
            self.title,
            self.post_date.unwrap_or_default()
        )
    }
}
