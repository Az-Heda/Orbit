-- Phase 0 foundation: settings + activity_log. Domain tables arrive in Phase 1.
-- Timestamps are UTC, RFC 3339 text.

CREATE TABLE settings (
    key        TEXT PRIMARY KEY NOT NULL,
    value      TEXT NOT NULL,              -- JSON-encoded value
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE activity_log (
    id          TEXT PRIMARY KEY NOT NULL, -- UUID
    entity_type TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    action      TEXT NOT NULL,             -- e.g. create | update | delete
    actor       TEXT NOT NULL CHECK (actor IN ('user', 'ai', 'automation')),
    before_json TEXT,
    after_json  TEXT,
    batch_id    TEXT,                      -- groups changes (e.g. one AI turn) for undo
    created_at  TEXT NOT NULL
) STRICT;

CREATE INDEX idx_activity_log_entity ON activity_log (entity_type, entity_id, created_at);
CREATE INDEX idx_activity_log_created ON activity_log (created_at);
CREATE INDEX idx_activity_log_batch ON activity_log (batch_id) WHERE batch_id IS NOT NULL;
