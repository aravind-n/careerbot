use std::error::Error;

use async_trait::async_trait;
use sqlx::PgPool;

use crate::{db::JobStore, job::Job};

/// A PostgreSQL-backed implementation of the `JobStore` trait.
///
/// This store provides methods to insert job records and check for their
/// existence in a PostgreSQL database using SQLx.
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    /// Creates a new `PostgresStore` with the given PostgreSQL connection pool.
    ///
    /// # Arguments
    ///
    /// * `pool` - A `PgPool` that will be used to execute database operations.
    ///
    /// # Returns
    ///
    /// * `PostgresStore` instance tied to the given pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl JobStore for PostgresStore {
    async fn insert_job(&self, job: &Job) -> Result<(), Box<dyn Error>> {
        sqlx::query!(
            r#"
            INSERT INTO jobs (
                id,
                job_portal_id,
                company_tag,
                title,
                description,
                employment_type,
                location,
                other_data,
                post_date,
                created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            job.id,
            job.job_portal_id,
            job.company_tag,
            job.title,
            job.description,
            job.employment_type,
            job.location.as_deref(),
            job.other_data.as_deref(),
            job.post_date,
            job.created_at,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn job_exists(&self, job: &Job) -> Result<bool, Box<dyn Error>> {
        let exists = sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM jobs
                WHERE job_portal_id = $1 AND company_tag = $2
            )
            "#,
            &job.job_portal_id,
            &job.company_tag,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(exists.unwrap_or(false))
    }
}
