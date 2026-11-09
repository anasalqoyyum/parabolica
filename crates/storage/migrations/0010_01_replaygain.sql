CREATE TABLE rg_scans (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    created_at_ms INTEGER NOT NULL,
    analyzer TEXT NOT NULL,
    target_lufs REAL NOT NULL CHECK(target_lufs=-18.0),
    peak_method TEXT NOT NULL CHECK(peak_method IN ('sample','true')),
    mode TEXT NOT NULL CHECK(mode IN ('tracks','album','tags','disc')),
    state TEXT NOT NULL CHECK(state IN ('queued','scanning','ready','incomplete','failed','cancelled','stale')),
    saved INTEGER NOT NULL DEFAULT 0 CHECK(saved IN (0,1)),
    error TEXT
) STRICT;
CREATE TABLE rg_groups (
    scan_id BLOB NOT NULL REFERENCES rg_scans(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    name TEXT NOT NULL,
    warning TEXT NOT NULL,
    stale INTEGER NOT NULL DEFAULT 0 CHECK(stale IN (0,1)),
    gain_db REAL,
    peak REAL,
    PRIMARY KEY(scan_id,ordinal)
) STRICT, WITHOUT ROWID;
CREATE TABLE rg_group_albums (
    scan_id BLOB NOT NULL,
    group_ordinal INTEGER NOT NULL,
    album_id BLOB NOT NULL CHECK(length(album_id)=16),
    PRIMARY KEY(scan_id,group_ordinal,album_id),
    FOREIGN KEY(scan_id,group_ordinal) REFERENCES rg_groups(scan_id,ordinal) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE TABLE rg_measurements (
    id INTEGER PRIMARY KEY,
    file_id BLOB NOT NULL CHECK(length(file_id)=16),
    digest BLOB NOT NULL CHECK(length(digest)=32),
    analyzer TEXT NOT NULL,
    peak_method TEXT NOT NULL CHECK(peak_method IN ('sample','true')),
    size_bytes INTEGER NOT NULL,
    modified_unix_nanos INTEGER NOT NULL,
    measured_at_ms INTEGER NOT NULL,
    sample_rate INTEGER NOT NULL,
    channel_mask INTEGER NOT NULL,
    frames INTEGER NOT NULL,
    sample_peak REAL NOT NULL,
    peak REAL NOT NULL,
    loudness_lufs REAL,
    energies BLOB NOT NULL,
    UNIQUE(file_id,digest,analyzer,peak_method)
) STRICT;
CREATE TABLE rg_members (
    scan_id BLOB NOT NULL,
    group_ordinal INTEGER NOT NULL,
    ordinal INTEGER NOT NULL,
    file_id BLOB NOT NULL CHECK(length(file_id)=16),
    title TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','done','failed')),
    detail TEXT,
    measurement_id INTEGER REFERENCES rg_measurements(id),
    reused INTEGER NOT NULL DEFAULT 0 CHECK(reused IN (0,1)),
    PRIMARY KEY(scan_id,group_ordinal,ordinal),
    UNIQUE(scan_id,file_id),
    FOREIGN KEY(scan_id,group_ordinal) REFERENCES rg_groups(scan_id,ordinal) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX rg_members_file ON rg_members(file_id);
CREATE INDEX rg_group_album ON rg_group_albums(album_id);

CREATE TRIGGER rg_membership_added AFTER INSERT ON local_songs BEGIN
    UPDATE rg_groups SET stale=1 WHERE (scan_id,ordinal) IN
        (SELECT scan_id,group_ordinal FROM rg_group_albums WHERE album_id=NEW.album_id);
END;
CREATE TRIGGER rg_membership_removed AFTER DELETE ON local_songs BEGIN
    UPDATE rg_groups SET stale=1 WHERE (scan_id,ordinal) IN
        (SELECT scan_id,group_ordinal FROM rg_group_albums WHERE album_id=OLD.album_id);
END;
CREATE TRIGGER rg_membership_changed AFTER UPDATE OF album_id ON local_songs
WHEN OLD.album_id IS NOT NEW.album_id BEGIN
    UPDATE rg_groups SET stale=1 WHERE (scan_id,ordinal) IN
        (SELECT scan_id,group_ordinal FROM rg_group_albums WHERE album_id IN (OLD.album_id,NEW.album_id));
END;
CREATE TRIGGER rg_file_changed AFTER UPDATE OF size_bytes,modified_unix_nanos,content_digest,presence ON media_files
WHEN OLD.size_bytes IS NOT NEW.size_bytes OR OLD.modified_unix_nanos IS NOT NEW.modified_unix_nanos
    OR OLD.content_digest IS NOT NEW.content_digest OR OLD.presence IS NOT NEW.presence BEGIN
    UPDATE rg_groups SET stale=1 WHERE (scan_id,ordinal) IN
        (SELECT scan_id,group_ordinal FROM rg_members WHERE file_id=NEW.id);
END;
CREATE TRIGGER rg_file_removed AFTER DELETE ON media_files BEGIN
    UPDATE rg_groups SET stale=1 WHERE (scan_id,ordinal) IN
        (SELECT scan_id,group_ordinal FROM rg_members WHERE file_id=OLD.id);
END;
