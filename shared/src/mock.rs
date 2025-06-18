use std::error::Error;

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    database::JobStore,
    job::{Job, JobBuilder},
    stream::StreamPublisher,
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
impl JobStore for MockJobStore {
    async fn insert_job(&self, _job: &Job) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    async fn job_exists(&self, job: &Job) -> Result<bool, Box<dyn Error>> {
        Ok(job.eq(&self.existing_job()))
    }
}

pub struct MockStreamPublisher;

#[async_trait]
impl StreamPublisher for MockStreamPublisher {
    async fn publish(&self, _message: Value) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}
