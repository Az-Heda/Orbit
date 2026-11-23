-- Global search: FTS5 indexes (external content, trigger-synced) for tasks, events, projects and inbox.
-- Notes' index lives in 0004. Soft-deleted rows are removed from the indexes.

-- Raw quick-capture items. The original text is never modified; triage (Phase 2) links the
-- resulting task/event back here. The table exists now so search can cover the inbox.
CREATE TABLE inbox_items (
    id                 TEXT PRIMARY KEY NOT NULL,
    raw_text           TEXT NOT NULL,
    status             TEXT NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'triaged', 'dismissed')),
    linked_entity_type TEXT CHECK (linked_entity_type IN ('task', 'event')),
    linked_entity_id   TEXT,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    deleted_at         TEXT
) STRICT;
CREATE INDEX idx_inbox_status ON inbox_items (status, created_at);

CREATE VIRTUAL TABLE tasks_fts USING fts5(
    title, description, content = 'tasks', content_rowid = 'rowid', tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER tasks_fts_ai AFTER INSERT ON tasks BEGIN
    INSERT INTO tasks_fts (rowid, title, description) SELECT new.rowid, new.title, new.description WHERE new.deleted_at IS NULL;
END;
CREATE TRIGGER tasks_fts_au AFTER UPDATE OF title, description, deleted_at ON tasks BEGIN
    INSERT INTO tasks_fts (tasks_fts, rowid, title, description) SELECT 'delete', old.rowid, old.title, old.description WHERE old.deleted_at IS NULL;
    INSERT INTO tasks_fts (rowid, title, description) SELECT new.rowid, new.title, new.description WHERE new.deleted_at IS NULL;
END;
CREATE TRIGGER tasks_fts_ad AFTER DELETE ON tasks BEGIN
    INSERT INTO tasks_fts (tasks_fts, rowid, title, description) SELECT 'delete', old.rowid, old.title, old.description WHERE old.deleted_at IS NULL;
END;
INSERT INTO tasks_fts (tasks_fts) VALUES ('rebuild');

CREATE VIRTUAL TABLE events_fts USING fts5(
    title, description, location, content = 'events', content_rowid = 'rowid', tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER events_fts_ai AFTER INSERT ON events BEGIN
    INSERT INTO events_fts (rowid, title, description, location) SELECT new.rowid, new.title, new.description, new.location WHERE new.deleted_at IS NULL;
END;
CREATE TRIGGER events_fts_au AFTER UPDATE OF title, description, location, deleted_at ON events BEGIN
    INSERT INTO events_fts (events_fts, rowid, title, description, location) SELECT 'delete', old.rowid, old.title, old.description, old.location WHERE old.deleted_at IS NULL;
    INSERT INTO events_fts (rowid, title, description, location) SELECT new.rowid, new.title, new.description, new.location WHERE new.deleted_at IS NULL;
END;
CREATE TRIGGER events_fts_ad AFTER DELETE ON events BEGIN
    INSERT INTO events_fts (events_fts, rowid, title, description, location) SELECT 'delete', old.rowid, old.title, old.description, old.location WHERE old.deleted_at IS NULL;
END;
INSERT INTO events_fts (events_fts) VALUES ('rebuild');

CREATE VIRTUAL TABLE projects_fts USING fts5(
    name, description, content = 'projects', content_rowid = 'rowid', tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER projects_fts_ai AFTER INSERT ON projects BEGIN
    INSERT INTO projects_fts (rowid, name, description) SELECT new.rowid, new.name, new.description WHERE new.deleted_at IS NULL;
END;
CREATE TRIGGER projects_fts_au AFTER UPDATE OF name, description, deleted_at ON projects BEGIN
    INSERT INTO projects_fts (projects_fts, rowid, name, description) SELECT 'delete', old.rowid, old.name, old.description WHERE old.deleted_at IS NULL;
    INSERT INTO projects_fts (rowid, name, description) SELECT new.rowid, new.name, new.description WHERE new.deleted_at IS NULL;
END;
CREATE TRIGGER projects_fts_ad AFTER DELETE ON projects BEGIN
    INSERT INTO projects_fts (projects_fts, rowid, name, description) SELECT 'delete', old.rowid, old.name, old.description WHERE old.deleted_at IS NULL;
END;
INSERT INTO projects_fts (projects_fts) VALUES ('rebuild');

CREATE VIRTUAL TABLE inbox_fts USING fts5(
    raw_text, content = 'inbox_items', content_rowid = 'rowid', tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER inbox_fts_ai AFTER INSERT ON inbox_items BEGIN
    INSERT INTO inbox_fts (rowid, raw_text) SELECT new.rowid, new.raw_text WHERE new.deleted_at IS NULL;
END;
CREATE TRIGGER inbox_fts_au AFTER UPDATE OF raw_text, deleted_at ON inbox_items BEGIN
    INSERT INTO inbox_fts (inbox_fts, rowid, raw_text) SELECT 'delete', old.rowid, old.raw_text WHERE old.deleted_at IS NULL;
    INSERT INTO inbox_fts (rowid, raw_text) SELECT new.rowid, new.raw_text WHERE new.deleted_at IS NULL;
END;
CREATE TRIGGER inbox_fts_ad AFTER DELETE ON inbox_items BEGIN
    INSERT INTO inbox_fts (inbox_fts, rowid, raw_text) SELECT 'delete', old.rowid, old.raw_text WHERE old.deleted_at IS NULL;
END;
INSERT INTO inbox_fts (inbox_fts) VALUES ('rebuild');
