use std::error::Error;

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    database::Database,
    job::{Job, JobBuilder},
    messaging::MessagePublisher,
    user::User,
};

#[derive(Debug)]
pub struct MockJobStore;

impl MockJobStore {
    pub fn existing_job(&self) -> Job {
        JobBuilder::default()
            .job_portal_id("EXISTS")
            .build()
            .unwrap()
    }
}

#[async_trait]
impl Database for MockJobStore {
    async fn insert_job(&self, _job: &Job) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    async fn get_interested_users_for_job(&self, _job: &Job) -> Result<Vec<User>, Box<dyn Error>> {
        Ok(Vec::new())
    }
}

pub struct MockStreamPublisher;

#[async_trait]
impl MessagePublisher for MockStreamPublisher {
    async fn publish(&self, _message: Value) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}
