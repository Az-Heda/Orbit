-- Calendar: events with recurrence on the series, per-occurrence exceptions,
-- and recurrence for tasks (the task's due_at is the series dtstart).

CREATE TABLE events (
    id              TEXT PRIMARY KEY NOT NULL,
    uid             TEXT NOT NULL,                 -- iCalendar UID, used to dedupe imports
    title           TEXT NOT NULL,
    description     TEXT,
    location        TEXT,
    all_day         INTEGER NOT NULL DEFAULT 0 CHECK (all_day IN (0, 1)),
    start_at        TEXT NOT NULL,                 -- UTC; for all-day: local midnight in `timezone`
    end_at          TEXT NOT NULL,                 -- UTC, exclusive for all-day events
    timezone        TEXT NOT NULL,                 -- IANA zone the series is expanded in
    recurrence_rule TEXT,                          -- bare RRULE value; dtstart = start_at in `timezone`
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    deleted_at      TEXT
) STRICT;
CREATE UNIQUE INDEX idx_events_uid ON events (uid) WHERE deleted_at IS NULL;
CREATE INDEX idx_events_start ON events (start_at);

CREATE TABLE event_exceptions (
    id             TEXT PRIMARY KEY NOT NULL,
    event_id       TEXT NOT NULL REFERENCES events (id),
    original_start TEXT NOT NULL,                  -- UTC start of the occurrence being changed
    kind           TEXT NOT NULL CHECK (kind IN ('skip', 'override')),
    title          TEXT,
    description    TEXT,
    location       TEXT,
    start_at       TEXT,
    end_at         TEXT,
    created_at     TEXT NOT NULL,
    UNIQUE (event_id, original_start)
) STRICT;

ALTER TABLE tasks ADD COLUMN recurrence_rule TEXT;
ALTER TABLE tasks ADD COLUMN recurrence_tz TEXT;

CREATE TABLE task_occurrences (
    task_id      TEXT NOT NULL REFERENCES tasks (id),
    original_due TEXT NOT NULL,
    status       TEXT NOT NULL CHECK (status IN ('done', 'skipped')),
    completed_at TEXT,
    PRIMARY KEY (task_id, original_due)
) STRICT;
