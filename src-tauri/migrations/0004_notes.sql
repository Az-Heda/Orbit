-- Notes and journal, linked polymorphically to tasks, events and projects.
-- FTS5 index (external content) is kept in sync by triggers; soft-deleted notes leave the index.

CREATE TABLE notes (
    id           TEXT PRIMARY KEY NOT NULL,
    title        TEXT NOT NULL,
    body         TEXT NOT NULL DEFAULT '',     -- markdown
    kind         TEXT NOT NULL DEFAULT 'note' CHECK (kind IN ('note', 'journal')),
    journal_date TEXT,                         -- local calendar date (yyyy-mm-dd) for journal entries
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    deleted_at   TEXT
) STRICT;
CREATE UNIQUE INDEX idx_notes_journal_date ON notes (journal_date)
    WHERE kind = 'journal' AND deleted_at IS NULL;
CREATE INDEX idx_notes_updated ON notes (updated_at);

CREATE TABLE note_links (
    note_id     TEXT NOT NULL REFERENCES notes (id),
    entity_type TEXT NOT NULL CHECK (entity_type IN ('task', 'event', 'project')),
    entity_id   TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    PRIMARY KEY (note_id, entity_type, entity_id)
) STRICT;
CREATE INDEX idx_note_links_entity ON note_links (entity_type, entity_id);

CREATE VIRTUAL TABLE notes_fts USING fts5(
    title, body, content = 'notes', content_rowid = 'rowid', tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER notes_fts_ai AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts (rowid, title, body) SELECT new.rowid, new.title, new.body WHERE new.deleted_at IS NULL;
END;
CREATE TRIGGER notes_fts_au AFTER UPDATE ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body)
        SELECT 'delete', old.rowid, old.title, old.body WHERE old.deleted_at IS NULL;
    INSERT INTO notes_fts (rowid, title, body) SELECT new.rowid, new.title, new.body WHERE new.deleted_at IS NULL;
END;
CREATE TRIGGER notes_fts_ad AFTER DELETE ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body)
        SELECT 'delete', old.rowid, old.title, old.body WHERE old.deleted_at IS NULL;
END;
