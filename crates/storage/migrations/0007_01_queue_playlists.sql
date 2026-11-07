CREATE TABLE external_files (
    id BLOB PRIMARY KEY CHECK (length(id) = 16),
    path TEXT NOT NULL UNIQUE CHECK (path <> '' AND instr(path, char(10)) = 0 AND instr(path, char(13)) = 0),
    added_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE playlists (
    id BLOB PRIMARY KEY CHECK (length(id) = 16),
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 256),
    created_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE playlist_entries (
    id BLOB PRIMARY KEY CHECK (length(id) = 16),
    playlist_id BLOB NOT NULL REFERENCES playlists (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('file', 'cue', 'provider')),
    source_id BLOB CHECK (source_id IS NULL OR length(source_id) = 16),
    provider TEXT,
    provider_item_id TEXT,
    CHECK (
        (source_kind IN ('file', 'cue') AND source_id IS NOT NULL AND provider IS NULL AND provider_item_id IS NULL)
        OR (source_kind = 'provider' AND source_id IS NULL AND provider IS NOT NULL AND provider_item_id IS NOT NULL AND provider_item_id <> '')
    )
) STRICT;
CREATE INDEX playlist_entries_order ON playlist_entries (playlist_id, position);

CREATE TABLE local_likes (
    source_key TEXT PRIMARY KEY,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('file', 'cue', 'provider')),
    source_id BLOB CHECK (source_id IS NULL OR length(source_id) = 16),
    provider TEXT,
    provider_item_id TEXT,
    liked_at_ms INTEGER NOT NULL,
    CHECK (
        (source_kind IN ('file', 'cue') AND source_id IS NOT NULL AND provider IS NULL AND provider_item_id IS NULL)
        OR (source_kind = 'provider' AND source_id IS NULL AND provider IS NOT NULL AND provider_item_id IS NOT NULL AND provider_item_id <> '')
    )
) STRICT;

CREATE TABLE queue_navigation (
    position INTEGER PRIMARY KEY CHECK (position >= 0),
    entry_id BLOB NOT NULL REFERENCES queue_entries (entry_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE queue_unavailable (
    entry_id BLOB PRIMARY KEY REFERENCES queue_entries (entry_id) ON DELETE CASCADE,
    reason TEXT NOT NULL,
    retry_attempts INTEGER NOT NULL CHECK (retry_attempts BETWEEN 0 AND 3)
) STRICT;

CREATE TABLE listening_history (
    position INTEGER PRIMARY KEY CHECK (position >= 0),
    entry_id BLOB NOT NULL CHECK (length(entry_id) = 16),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('file', 'cue', 'provider')),
    source_id BLOB CHECK (source_id IS NULL OR length(source_id) = 16),
    provider TEXT,
    provider_item_id TEXT,
    started_at_ms INTEGER NOT NULL,
    CHECK (
        (source_kind IN ('file', 'cue') AND source_id IS NOT NULL AND provider IS NULL AND provider_item_id IS NULL)
        OR (source_kind = 'provider' AND source_id IS NULL AND provider IS NOT NULL AND provider_item_id IS NOT NULL AND provider_item_id <> '')
    )
) STRICT;
