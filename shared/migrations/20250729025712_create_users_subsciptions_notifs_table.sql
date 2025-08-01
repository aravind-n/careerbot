-- Add migration script here
ALTER TABLE jobs
ADD UNIQUE(company_tag, job_portal_id);

CREATE TABLE users (
    id UUID PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE subscriptions (
    id UUID primary key,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    company_tag TEXT NOT NULL, -- e.g. 'MSFT', 'GOOG'
    query_string TEXT[], -- e.g. ['SDE', 'Applied Scientist']
    exclude_string TEXT[], -- Exclude jobs that contains these
    created_at TIMESTAMPTZ DEFAULT now(),
    UNIQUE(user_id, company_tag)
);

CREATE TABLE notification_history (
    id UUID primary key,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    sent_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now()
);