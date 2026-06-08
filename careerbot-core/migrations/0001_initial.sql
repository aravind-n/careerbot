-- Initial schema for careerbot — see PLAN.md §6.
--
-- `jobs` is the canonical dedup target: (company_tag, external_id) is unique,
-- and `first_seen_at` drives "new vs. known" decisions in the notification
-- dispatcher.
CREATE TABLE jobs (
    id              INTEGER PRIMARY KEY,
    company_tag     TEXT NOT NULL,
    external_id     TEXT NOT NULL,
    title           TEXT NOT NULL,
    url             TEXT NOT NULL,
    location        TEXT,
    posted_at       TEXT,
    description     TEXT,
    first_seen_at   TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(company_tag, external_id)
);

-- Per-tick audit trail for each per-company script run. `exit_code` ≠ 0
-- (and `error` non-null) is the canonical breakage signal that drives the
-- repair pipeline.
CREATE TABLE runs (
    id                  INTEGER PRIMARY KEY,
    company_tag         TEXT NOT NULL,
    started_at          TEXT NOT NULL,
    finished_at         TEXT,
    exit_code           INTEGER,
    new_job_count       INTEGER,
    stderr_tail         TEXT,
    error               TEXT
);

-- One row per notification attempt. ON DELETE CASCADE keeps the history
-- consistent when a job is removed (e.g. company purged).
CREATE TABLE notifications (
    id          INTEGER PRIMARY KEY,
    job_id      INTEGER NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    channel     TEXT NOT NULL,
    sent_at     TEXT NOT NULL,
    success     INTEGER NOT NULL,
    error       TEXT
);

-- Per-invocation token accounting across providers. Lets the daemon expose
-- "what did this cost me" without parsing provider-specific responses.
CREATE TABLE token_usage (
    id              INTEGER PRIMARY KEY,
    occurred_at     TEXT NOT NULL,
    provider        TEXT NOT NULL,
    model           TEXT,
    purpose         TEXT NOT NULL,
    input_tokens    INTEGER,
    output_tokens   INTEGER,
    company_tag     TEXT
);

-- Per-company scheduling and repair state. `needs_attention = 1` parks the
-- company until manual intervention clears it.
CREATE TABLE company_state (
    company_tag             TEXT PRIMARY KEY,
    last_tick_at            TEXT,
    consecutive_failures    INTEGER NOT NULL DEFAULT 0,
    needs_attention         INTEGER NOT NULL DEFAULT 0
);
