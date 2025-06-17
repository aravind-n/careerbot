-- Add migration script here
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE jobs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    job_portal_id TEXT NOT NULL,
    company_tag TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT,
    employment_type TEXT,
    location TEXT[],
    other_data TEXT[],
    post_date TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL
);
