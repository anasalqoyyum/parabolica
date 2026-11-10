-- Reviewed album tag and artwork writes (#12). Each write is a `jobs` row of kind 'tag-write'.
-- The reviewed patch for every file is kept so a failed or deferred file can be retried after a
-- restart, and the per-file state records backups and the content written. File and album IDs are
-- not foreign keys: the history outlives a file's removal from the library.

CREATE TABLE tag_writes (
    job_id BLOB PRIMARY KEY REFERENCES jobs (id) ON DELETE CASCADE,
    album_id BLOB NOT NULL CHECK (length(album_id) = 16),
    retry_of BLOB REFERENCES jobs (id) ON DELETE SET NULL,
    backups_discarded INTEGER NOT NULL DEFAULT 0 CHECK (backups_discarded IN (0, 1))
) STRICT;

CREATE INDEX tag_writes_by_album ON tag_writes (album_id);

-- Validated images copied into the app's staging folder at review.
CREATE TABLE tag_write_images (
    job_id BLOB NOT NULL REFERENCES tag_writes (job_id) ON DELETE CASCADE,
    artwork_id BLOB NOT NULL CHECK (length(artwork_id) = 16),
    staged_path TEXT NOT NULL CHECK (staged_path <> ''),
    PRIMARY KEY (job_id, artwork_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE tag_write_targets (
    job_id BLOB NOT NULL REFERENCES tag_writes (job_id) ON DELETE CASCADE,
    file_id BLOB NOT NULL CHECK (length(file_id) = 16),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    path TEXT NOT NULL CHECK (path <> ''),
    reviewed_size_bytes INTEGER NOT NULL CHECK (reviewed_size_bytes >= 0),
    reviewed_modified_unix_nanos INTEGER NOT NULL,
    reviewed_digest BLOB NOT NULL CHECK (length(reviewed_digest) = 32),
    state TEXT NOT NULL CHECK (state IN (
        'pending', 'replacing', 'written', 'unchanged', 'changed', 'deferred', 'failed', 'restored'
    )),
    detail TEXT,
    backup_path TEXT,
    result_size_bytes INTEGER,
    result_modified_unix_nanos INTEGER,
    result_digest BLOB CHECK (result_digest IS NULL OR length(result_digest) = 32),
    PRIMARY KEY (job_id, file_id),
    UNIQUE (job_id, ordinal),
    CHECK ((state IN ('deferred', 'failed')) = (detail IS NOT NULL)),
    CHECK ((state IN ('replacing', 'written')) = (backup_path IS NOT NULL)),
    CHECK ((state IN ('replacing', 'written', 'restored')) = (result_digest IS NOT NULL)),
    CHECK ((result_digest IS NULL) = (result_size_bytes IS NULL)),
    CHECK ((result_size_bytes IS NULL) = (result_modified_unix_nanos IS NULL))
) STRICT, WITHOUT ROWID;

CREATE TABLE tag_write_fields (
    job_id BLOB NOT NULL,
    file_id BLOB NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    field TEXT NOT NULL CHECK (field <> ''),
    edit TEXT NOT NULL CHECK (edit IN ('replace', 'fill', 'remove')),
    PRIMARY KEY (job_id, file_id, ordinal),
    FOREIGN KEY (job_id, file_id) REFERENCES tag_write_targets (job_id, file_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

CREATE TABLE tag_write_values (
    job_id BLOB NOT NULL,
    file_id BLOB NOT NULL,
    field_ordinal INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    value TEXT NOT NULL,
    PRIMARY KEY (job_id, file_id, field_ordinal, ordinal),
    FOREIGN KEY (job_id, file_id, field_ordinal)
        REFERENCES tag_write_fields (job_id, file_id, ordinal) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

CREATE TABLE tag_write_artwork (
    job_id BLOB NOT NULL,
    file_id BLOB NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    artwork_id BLOB NOT NULL,
    picture_type TEXT NOT NULL CHECK (picture_type IN ('front', 'back', 'booklet', 'other')),
    destination TEXT NOT NULL CHECK (destination IN ('embed', 'beside', 'both')),
    PRIMARY KEY (job_id, file_id, ordinal),
    FOREIGN KEY (job_id, file_id) REFERENCES tag_write_targets (job_id, file_id) ON DELETE CASCADE,
    FOREIGN KEY (job_id, artwork_id) REFERENCES tag_write_images (job_id, artwork_id)
) STRICT, WITHOUT ROWID;

-- `replace_names` holds the reviewed existing sidecar names joined with '/', which no file name
-- contains.
CREATE TABLE tag_write_sidecars (
    job_id BLOB NOT NULL REFERENCES tag_writes (job_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    directory TEXT NOT NULL CHECK (directory <> ''),
    file_name TEXT NOT NULL CHECK (file_name <> '' AND instr(file_name, '/') = 0),
    picture_type TEXT NOT NULL CHECK (picture_type IN ('front', 'back', 'booklet')),
    artwork_id BLOB NOT NULL,
    replace_names TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'written', 'changed', 'failed')),
    detail TEXT,
    PRIMARY KEY (job_id, ordinal),
    FOREIGN KEY (job_id, artwork_id) REFERENCES tag_write_images (job_id, artwork_id),
    CHECK ((state = 'failed') = (detail IS NOT NULL))
) STRICT, WITHOUT ROWID;
