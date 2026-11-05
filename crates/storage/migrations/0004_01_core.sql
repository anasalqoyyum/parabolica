-- Core schema owned by #4: file identity, display tags, CUE ranges, provider items, source links,
-- preferences, the queue checkpoint, jobs and library search. Never edit after merge; add a new
-- migration instead.

CREATE TABLE library_roots (
    id BLOB PRIMARY KEY CHECK (length(id) = 16),
    path TEXT NOT NULL UNIQUE CHECK (path <> ''),
    volume_id TEXT,
    availability TEXT NOT NULL CHECK (availability IN ('online', 'offline', 'permission-denied')),
    added_at_ms INTEGER NOT NULL
) STRICT;

-- Physical file identity. `seq` is a stable rowid alias used as the search row key; VACUUM can
-- renumber implicit rowids.
CREATE TABLE media_files (
    seq INTEGER PRIMARY KEY,
    id BLOB NOT NULL UNIQUE CHECK (length(id) = 16),
    root_id BLOB NOT NULL REFERENCES library_roots (id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL CHECK (relative_path <> ''),
    os_volume_id TEXT,
    os_file_id BLOB CHECK (os_file_id IS NULL OR length(os_file_id) = 16),
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    modified_unix_nanos INTEGER NOT NULL,
    content_digest BLOB CHECK (content_digest IS NULL OR length(content_digest) = 32),
    presence TEXT NOT NULL CHECK (presence IN ('present', 'missing')),
    indexed_at_ms INTEGER NOT NULL,
    UNIQUE (root_id, relative_path),
    CHECK ((os_volume_id IS NULL) = (os_file_id IS NULL))
) STRICT;

-- Not unique: hard links share one OS file ID.
CREATE INDEX media_files_os_identity ON media_files (os_volume_id, os_file_id)
    WHERE os_file_id IS NOT NULL;

-- Display tags as read from the file. Multi-value fields keep their order through `ordinal`.
CREATE TABLE media_file_tags (
    file_id BLOB NOT NULL REFERENCES media_files (id) ON DELETE CASCADE,
    field TEXT NOT NULL CHECK (field <> '' AND field = upper(field)),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    value TEXT NOT NULL,
    PRIMARY KEY (file_id, field, ordinal)
) STRICT, WITHOUT ROWID;

CREATE TABLE virtual_tracks (
    id BLOB PRIMARY KEY CHECK (length(id) = 16),
    image_file_id BLOB NOT NULL REFERENCES media_files (id) ON DELETE CASCADE,
    number INTEGER NOT NULL CHECK (number BETWEEN 1 AND 99),
    start_frame INTEGER NOT NULL CHECK (start_frame >= 0),
    end_frame INTEGER NOT NULL CHECK (end_frame > start_frame),
    index_01_frame INTEGER NOT NULL CHECK (index_01_frame >= start_frame AND index_01_frame < end_frame),
    UNIQUE (image_file_id, number)
) STRICT;

CREATE TABLE provider_items (
    provider TEXT NOT NULL CHECK (provider <> ''),
    item_id TEXT NOT NULL CHECK (item_id <> ''),
    first_seen_at_ms INTEGER NOT NULL,
    PRIMARY KEY (provider, item_id)
) STRICT, WITHOUT ROWID;

-- A logical recording. Playable sources link to it only after confirmation or as a reversible
-- suggestion; a rejected suggestion is kept so it is not proposed again.
CREATE TABLE tracks (
    id BLOB PRIMARY KEY CHECK (length(id) = 16),
    created_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE track_sources (
    track_id BLOB NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('file', 'cue', 'provider')),
    file_id BLOB REFERENCES media_files (id) ON DELETE CASCADE,
    virtual_track_id BLOB REFERENCES virtual_tracks (id) ON DELETE CASCADE,
    provider TEXT,
    provider_item_id TEXT,
    link_state TEXT NOT NULL CHECK (link_state IN ('confirmed', 'suggested', 'rejected')),
    evidence TEXT,
    linked_at_ms INTEGER NOT NULL,
    source_key TEXT NOT NULL GENERATED ALWAYS AS (
        source_kind || ':' || coalesce(hex(file_id), hex(virtual_track_id), provider || ':' || provider_item_id)
    ) VIRTUAL,
    FOREIGN KEY (provider, provider_item_id) REFERENCES provider_items (provider, item_id) ON DELETE CASCADE,
    CHECK (
        (source_kind = 'file' AND file_id IS NOT NULL AND virtual_track_id IS NULL AND provider IS NULL AND provider_item_id IS NULL)
        OR (source_kind = 'cue' AND file_id IS NULL AND virtual_track_id IS NOT NULL AND provider IS NULL AND provider_item_id IS NULL)
        OR (source_kind = 'provider' AND file_id IS NULL AND virtual_track_id IS NULL AND provider IS NOT NULL AND provider_item_id IS NOT NULL)
    )
) STRICT;

-- A source plays one recording; rejections are per track.
CREATE UNIQUE INDEX track_sources_one_track ON track_sources (source_key) WHERE link_state <> 'rejected';
CREATE UNIQUE INDEX track_sources_pair ON track_sources (source_key, track_id);
CREATE INDEX track_sources_by_track ON track_sources (track_id);

CREATE TABLE preferences (
    key TEXT PRIMARY KEY CHECK (key <> ''),
    value TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

-- The play queue checkpoint. Entries keep their source even when it becomes unavailable, so they
-- do not reference library tables.
CREATE TABLE queue_entries (
    position INTEGER PRIMARY KEY CHECK (position >= 0),
    entry_id BLOB NOT NULL UNIQUE CHECK (length(entry_id) = 16),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('file', 'cue', 'provider')),
    source_id BLOB CHECK (source_id IS NULL OR length(source_id) = 16),
    provider TEXT,
    provider_item_id TEXT,
    shuffle_position INTEGER UNIQUE CHECK (shuffle_position IS NULL OR shuffle_position >= 0),
    CHECK (
        (source_kind IN ('file', 'cue') AND source_id IS NOT NULL AND provider IS NULL AND provider_item_id IS NULL)
        OR (source_kind = 'provider' AND source_id IS NULL AND provider IS NOT NULL AND provider_item_id IS NOT NULL)
    )
) STRICT;

CREATE TABLE queue_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    current_entry_id BLOB REFERENCES queue_entries (entry_id) DEFERRABLE INITIALLY DEFERRED,
    position_micros INTEGER NOT NULL CHECK (position_micros >= 0),
    repeat_mode TEXT NOT NULL CHECK (repeat_mode IN ('off', 'all', 'one')),
    shuffle_seed INTEGER,
    saved_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE jobs (
    id BLOB PRIMARY KEY CHECK (length(id) = 16),
    kind TEXT NOT NULL CHECK (kind <> ''),
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'completed', 'incomplete', 'failed', 'stale', 'cancelled')),
    progress_done INTEGER NOT NULL CHECK (progress_done >= 0),
    progress_total INTEGER CHECK (progress_total IS NULL OR progress_total >= 0),
    failure_reason TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    CHECK ((state = 'failed') = (failure_reason IS NOT NULL))
) STRICT;

CREATE INDEX jobs_unfinished ON jobs (created_at_ms) WHERE state IN ('queued', 'running');

-- Units of a resumable job (a file to scan or write). A resumed job reruns items that are still
-- pending, and items whose recorded signature no longer matches the file.
CREATE TABLE job_items (
    job_id BLOB NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    item_key TEXT NOT NULL CHECK (item_key <> ''),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    file_id BLOB REFERENCES media_files (id) ON DELETE SET NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('pending', 'done', 'skipped', 'failed')),
    detail TEXT,
    signature_size_bytes INTEGER,
    signature_modified_unix_nanos INTEGER,
    signature_digest BLOB CHECK (signature_digest IS NULL OR length(signature_digest) = 32),
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (job_id, item_key),
    UNIQUE (job_id, ordinal),
    CHECK ((signature_size_bytes IS NULL) = (signature_modified_unix_nanos IS NULL)),
    CHECK (signature_digest IS NULL OR signature_size_bytes IS NOT NULL)
) STRICT, WITHOUT ROWID;

-- Library search over display tags. The trigram tokenizer matches substrings in any script,
-- including CJK text without word breaks. Rows are keyed by media_files.seq and maintained by the
-- storage crate in the same transaction as the tags.
CREATE VIRTUAL TABLE media_file_search USING fts5 (
    title,
    artist,
    album,
    album_artist,
    other,
    tokenize = 'trigram remove_diacritics 1'
);

CREATE TRIGGER media_files_search_delete AFTER DELETE ON media_files BEGIN
    DELETE FROM media_file_search WHERE rowid = old.seq;
END;
