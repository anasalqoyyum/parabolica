CREATE TABLE rg_publications (
    file_id BLOB PRIMARY KEY CHECK(length(file_id)=16),
    scan_id BLOB NOT NULL CHECK(length(scan_id)=16),
    FOREIGN KEY(scan_id,file_id) REFERENCES rg_members(scan_id,file_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

INSERT INTO rg_publications(file_id,scan_id)
SELECT file_id,scan_id FROM (
    SELECT m.file_id,s.id AS scan_id,
        row_number() OVER (PARTITION BY m.file_id ORDER BY s.created_at_ms DESC,s.id DESC) AS selection
    FROM rg_scans s JOIN rg_members m ON m.scan_id=s.id
    WHERE s.saved=1
) WHERE selection=1;
