-- Read-only subscribed calendars (ICS feeds). The feed URL is a secret (it often carries a private
-- token) and lives in the OS credential store, never in this table.
CREATE TABLE external_calendars (
    id             TEXT PRIMARY KEY NOT NULL,
    name           TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    last_synced_at TEXT,
    last_error     TEXT,
    deleted_at     TEXT
) STRICT;

ALTER TABLE events ADD COLUMN external_calendar_id TEXT REFERENCES external_calendars (id);
CREATE INDEX idx_events_external ON events (external_calendar_id);
