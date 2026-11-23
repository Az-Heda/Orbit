-- Phase 3: Claude copilot. Conversations, proposals (pending AI changes) and AI memory.
-- The API key is NOT stored here (Windows Credential Manager only).

CREATE TABLE ai_conversations (
    id         TEXT PRIMARY KEY NOT NULL,
    title      TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;

-- `content_json` is the exact array of content blocks sent to / received from the API, so
-- thinking and tool_use blocks can be replayed unchanged.
CREATE TABLE ai_messages (
    id              TEXT PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES ai_conversations (id) ON DELETE CASCADE,
    seq             INTEGER NOT NULL,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content_json    TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    UNIQUE (conversation_id, seq)
) STRICT;

-- A mutating tool call waits here until the user confirms it. Nothing is applied before that.
CREATE TABLE ai_proposals (
    id              TEXT PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES ai_conversations (id) ON DELETE CASCADE,
    tool_name       TEXT NOT NULL,
    input_json      TEXT NOT NULL,
    preview_json    TEXT NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('pending', 'applied', 'rejected', 'failed', 'undone')),
    result_json     TEXT,
    reported        INTEGER NOT NULL DEFAULT 0,  -- already told to the model on a later turn
    created_at      TEXT NOT NULL,
    resolved_at     TEXT
) STRICT;
CREATE INDEX idx_ai_proposals_conv ON ai_proposals (conversation_id, created_at);

-- Plain, user-visible text. Editable and deletable in Settings.
CREATE TABLE ai_memories (
    id         TEXT PRIMARY KEY NOT NULL,
    text       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
) STRICT;
