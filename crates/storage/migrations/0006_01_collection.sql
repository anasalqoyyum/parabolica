CREATE TABLE local_albums (
    id BLOB PRIMARY KEY CHECK (length(id) = 16),
    group_key TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    edition TEXT NOT NULL
) STRICT;

CREATE TABLE local_artists (
    id BLOB PRIMARY KEY CHECK (length(id) = 16),
    name TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE local_songs (
    file_id BLOB PRIMARY KEY REFERENCES media_files(id) ON DELETE CASCADE,
    album_id BLOB NOT NULL REFERENCES local_albums(id),
    auto_group_key TEXT NOT NULL,
    manual_group INTEGER NOT NULL DEFAULT 0 CHECK (manual_group IN (0, 1)),
    needs_refresh INTEGER NOT NULL DEFAULT 0 CHECK (needs_refresh IN (0, 1)),
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    folder TEXT NOT NULL,
    disc INTEGER NOT NULL,
    track INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    format TEXT NOT NULL,
    metadata_error TEXT
) STRICT;
CREATE INDEX local_songs_album ON local_songs(album_id, disc, track, title);
CREATE INDEX local_songs_folder ON local_songs(folder);

CREATE TRIGGER local_song_invalidate AFTER UPDATE OF root_id, relative_path,
    size_bytes, modified_unix_nanos, content_digest, os_volume_id, os_file_id ON media_files
BEGIN
    UPDATE local_songs SET needs_refresh=1 WHERE file_id=NEW.id;
END;

CREATE TABLE local_song_artists (
    file_id BLOB NOT NULL REFERENCES local_songs(file_id) ON DELETE CASCADE,
    artist_id BLOB NOT NULL REFERENCES local_artists(id),
    PRIMARY KEY(file_id, artist_id)
) STRICT, WITHOUT ROWID;
