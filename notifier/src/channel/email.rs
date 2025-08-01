use std::error::Error;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};
use shared::{job::Job, user::User};
use tracing::error;

use crate::channel::NotificationChannel;

pub(crate) struct EmailChannel {
    api_key: String,
    from_address: String,
    client: Client,
}

impl EmailChannel {
    pub fn new(api_key: String, from_address: String) -> Self {
        Self {
            api_key,
            from_address,
            client: Client::new(),
        }
    }

    fn build_payload(&self, user: &User, job: &Job) -> Value {
        json!({
            "personalizations": [{
                "to": [{ "email": user.email }],
                "subject": format!("New Job Alert: {}", job.company_tag),
            }],
            "from": [{ "email": self.from_address }],
            "content": [{
                "type": "text/plain",
                "value": format!("New job posted for {}", job.to_string()),
            }]
        })
    }
}

#[async_trait]
impl NotificationChannel for EmailChannel {
    async fn send(&self, user: &User, job: &Job) -> Result<(), Box<dyn Error>> {
        let payload = self.build_payload(user, job);

        let res = self
            .client
            .post("https://api.sendgrid.com/v3/mail/send")
            .bearer_auth(self.api_key.clone())
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                error!(
                    error = %e,
                    user = %user,
                    job = %job,
                    "Error occured while communicating with email backend",
                );
                e
            })?;

        if !res.status().is_success() {
            error!(status = %res.status(), "Sendgrid failure");
            return Err("Sendgrid Error".into());
        }

        Ok(())
    }

    fn name(&self) -> String {
        "email".into()
    }
}
