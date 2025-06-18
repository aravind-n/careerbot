use std::error::Error;

use async_trait::async_trait;
use chrono::DateTime;
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::Value;
use shared::job::{Job, JobBuilder};
use tracing::{error, info};

use crate::collector::{Collector, JobCollector};

/// Represents a collector for job listings from Microsoft's career API.
///
/// Responsible for constructing API requests, handling responses,
/// and converting job data into the internal `Job` format.
pub struct MicrosoftCollector {
    endpoint: String,
}

impl MicrosoftCollector {
    /// Creates a new MicrosoftCollector
    ///
    /// Sets the endpoint URL to the Microsoft careers
    /// website. This lets the Collector fetch jobs from
    /// the correct source
    pub fn new() -> Self {
        Self {
            endpoint: String::from(
                "https://gcsservices.careers.microsoft.com/search/api/v1/search",
            ),
        }
    }
}

#[async_trait]
impl JobCollector for MicrosoftCollector {
    /// Attempts to fetch job data from Microsoft's job search API.
    ///
    /// Constructs the endpoint URL with query parameters, sends a GET request,
    /// and parses the JSON response into a `serde_json::Value`.
    async fn fetch_api_response(&self) -> Result<Value, Box<dyn Error>> {
        let endpoint_url = Url::parse_with_params(
            &self.endpoint,
            &[
                ("pg", "1"),
                ("pgSz", "20"),
                ("o", "Recent"),
                ("flt", "true"),
            ],
        )
        .inspect_err(|e| error!(error = %e, "Unable to parse URL"))?;

        info!(collector = %self.name(), "Querying API");

        Client::new()
            .get(endpoint_url.clone())
            .send()
            .await?
            .json::<Value>()
            .await
            .map_err(|e| {
                error!(error = %e, "Unable to parse JSON");
                e.into()
            })
    }

    /// Parses the API response JSON into a list of internal `Job` representations.
    ///
    /// Extracts job entries from the nested JSON structure, attempts to deserialize
    /// each job into a `MicrosoftJobJson`, and converts it into a `Job`.
    fn process(&self, api_response: Value) -> Result<Vec<Job>, Box<dyn Error>> {
        let job_list = api_response
            .get("operationResult")
            .and_then(|value| value.get("result"))
            .and_then(|value| value.get("jobs"))
            .ok_or("ERROR: Failed to extract jobs")?;

        job_list
            .as_array()
            .ok_or("ERROR: Unable to parse job list")?
            .iter()
            .map(|value| {
                serde_json::from_value::<MicrosoftJobJson>(value.clone())
                    .map_err(|e| {
                        error!(error = %e, "Deserialize error");
                        format!("Deserialize error: {e}")
                    })?
                    .try_into()
            })
            .collect()
    }

    fn name(&self) -> &'static str {
        Collector::Microsoft.as_str()
    }
}

/// Intermediate representation of a job entry returned by the Microsoft careers API.
///
/// This struct is deserialized from the raw JSON response and contains
/// key job metadata, including the ID, title, and a nested `properties` object.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MicrosoftJobJson {
    job_id: String,
    title: String,
    properties: MicrosoftJobJsonProperties,
    posting_date: String,
}

/// Represents the detailed properties of a Microsoft job entry.
///
/// Includes job attributes such as description, discipline, type,
/// location, and flexibility options.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct MicrosoftJobJsonProperties {
    description: String,
    discipline: String,
    // FIXME: Deserialization fails when education_level is missing
    // education_level: String,
    employment_type: String,
    job_type: String,
    locations: Vec<String>,
    primary_location: String,
    profession: String,
    role_type: String,
    work_site_flexibility: String,
}

/// Attempts to convert a `MicrosoftJobJson` into the internal `Job` format.
///
/// This involves parsing dates, flattening nested fields, and preparing
/// data for use within the broader job processing pipeline.
impl TryFrom<MicrosoftJobJson> for Job {
    type Error = Box<dyn Error>;

    fn try_from(value: MicrosoftJobJson) -> Result<Self, Self::Error> {
        let tag = "MSFT";

        Ok(JobBuilder::default()
            .job_portal_id(value.job_id)
            .company_tag(tag)
            .title(value.title)
            .post_date(DateTime::parse_from_rfc3339(&value.posting_date)?)
            .description(value.properties.description)
            .employment_type(value.properties.employment_type)
            .location(value.properties.locations)
            .other_data(vec![
                value.properties.job_type,
                value.properties.primary_location,
                value.properties.discipline,
                value.properties.profession,
                value.properties.role_type,
                value.properties.work_site_flexibility,
            ])
            .build()?)
    }
}
