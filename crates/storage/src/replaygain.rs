//! Scanner snapshots, immutable complete track cache and explicitly published app results.

use crate::codec::{blob16, now_ms, signature_from_sql, signature_to_sql};
use crate::{Storage, StorageError};
use parabolica_domain::files::FileSignature;
use parabolica_domain::gain::{
    PeakMethod,
    analysis::{ANALYZER, AlbumAnalysis, TARGET_LUFS, TrackAnalysis},
};
use parabolica_domain::ids::{JobId, MediaFileId, ReleaseId};
use rusqlite::{OptionalExtension, Row, params};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    Tracks,
    Album,
    Tags,
    Disc,
}
impl ScanMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tracks => "Scan tracks",
            Self::Album => "Scan as one album",
            Self::Tags => "Scan albums by tags",
            Self::Disc => "Group by disc",
        }
    }
    fn sql(self) -> &'static str {
        match self {
            Self::Tracks => "tracks",
            Self::Album => "album",
            Self::Tags => "tags",
            Self::Disc => "disc",
        }
    }
    fn parse(s: &str) -> Result<Self, StorageError> {
        match s {
            "tracks" => Ok(Self::Tracks),
            "album" => Ok(Self::Album),
            "tags" => Ok(Self::Tags),
            "disc" => Ok(Self::Disc),
            _ => Err(invalid()),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanState {
    Queued,
    Scanning,
    Ready,
    Incomplete,
    Failed,
    Stale,
    Cancelled,
}
impl ScanState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Scanning => "Scanning",
            Self::Ready => "Ready",
            Self::Incomplete => "Incomplete",
            Self::Failed => "Failed",
            Self::Stale => "Stale",
            Self::Cancelled => "Cancelled",
        }
    }
    fn sql(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Scanning => "scanning",
            Self::Ready => "ready",
            Self::Incomplete => "incomplete",
            Self::Failed => "failed",
            Self::Stale => "stale",
            Self::Cancelled => "cancelled",
        }
    }
    fn parse(s: &str) -> Result<Self, StorageError> {
        match s {
            "queued" => Ok(Self::Queued),
            "scanning" => Ok(Self::Scanning),
            "ready" => Ok(Self::Ready),
            "incomplete" => Ok(Self::Incomplete),
            "failed" => Ok(Self::Failed),
            "stale" => Ok(Self::Stale),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(invalid()),
        }
    }
}
#[derive(Debug, Clone)]
pub struct ScanCandidate {
    pub file: MediaFileId,
    pub album: ReleaseId,
    pub path: PathBuf,
    pub title: String,
    pub album_title: String,
    pub album_artist: String,
    pub edition: String,
    pub disc: u32,
    pub track: u32,
    pub available: bool,
}
#[derive(Debug, Clone)]
pub struct ScanMember {
    pub file: MediaFileId,
    pub title: String,
}
#[derive(Debug, Clone)]
pub struct ReviewedGroup {
    pub name: String,
    pub warning: String,
    pub members: Vec<ScanMember>,
    /// Full library membership at review time, even when accepting only a subset.
    pub albums: Vec<(ReleaseId, Vec<MediaFileId>)>,
}
#[derive(Debug, Clone)]
pub struct ScanRecord {
    pub id: JobId,
    pub created_at_ms: i64,
    pub analyzer: String,
    pub target_lufs: f64,
    pub method: PeakMethod,
    pub mode: ScanMode,
    pub state: ScanState,
    pub saved: bool,
    pub error: Option<String>,
    pub groups: Vec<GroupResult>,
}
#[derive(Debug, Clone)]
pub struct GroupResult {
    pub reviewed: ReviewedGroup,
    pub stale: bool,
    pub gain_db: Option<f64>,
    pub peak: Option<f64>,
    pub members: Vec<MemberResult>,
}
#[derive(Debug, Clone)]
pub struct MemberResult {
    pub file: MediaFileId,
    pub title: String,
    pub done: bool,
    pub detail: Option<String>,
    pub measurement: Option<MeasurementSummary>,
    pub reused: bool,
}
#[derive(Debug, Clone)]
pub struct PublishedAnalysis {
    pub scan: JobId,
    pub analyzer: String,
    pub target_lufs: f64,
    pub track: CachedAnalysis,
    pub album_gain_db: Option<f64>,
    pub album_peak: Option<f64>,
    pub stale: bool,
}

#[derive(Debug, Clone)]
pub struct MeasurementSummary {
    pub id: i64,
    pub signature: FileSignature,
    pub loudness_lufs: Option<f64>,
    pub peak: f64,
}
impl MeasurementSummary {
    pub fn gain_db(&self) -> Option<f64> {
        self.loudness_lufs.map(|loudness| TARGET_LUFS - loudness)
    }
}

#[derive(Debug, Clone)]
pub struct CachedAnalysis {
    pub id: i64,
    pub signature: FileSignature,
    pub analysis: TrackAnalysis,
}
fn invalid() -> StorageError {
    StorageError::InvalidStoredValue {
        column: "ReplayGain measurement",
    }
}
fn peak_sql(m: PeakMethod) -> &'static str {
    match m {
        PeakMethod::Sample => "sample",
        PeakMethod::TruePeak => "true",
    }
}
fn peak_parse(s: &str) -> Result<PeakMethod, StorageError> {
    match s {
        "sample" => Ok(PeakMethod::Sample),
        "true" => Ok(PeakMethod::TruePeak),
        _ => Err(invalid()),
    }
}

impl Storage {
    pub fn scan_candidates(&self) -> Result<Vec<ScanCandidate>, StorageError> {
        self.read(|tx| {
            let mut stmt = tx.prepare("SELECT f.id,s.album_id,r.path,f.relative_path,s.title,a.title,a.artist,a.edition,s.disc,s.track,f.presence='present' AND r.availability='online' AND s.metadata_error IS NULL FROM local_songs s JOIN local_albums a ON a.id=s.album_id JOIN media_files f ON f.id=s.file_id JOIN library_roots r ON r.id=f.root_id ORDER BY a.artist,a.title,a.id,s.disc,s.track,s.title,f.id")?;
            let mut rows = stmt.query([])?; let mut items = Vec::new();
            while let Some(r) = rows.next()? { items.push(ScanCandidate { file: MediaFileId::from_bytes(blob16(r,0,"file_id")?), album: ReleaseId::from_bytes(blob16(r,1,"album_id")?), path: PathBuf::from(r.get::<_,String>(2)?).join(crate::codec::relative_path_from_sql(&r.get::<_,String>(3)?)), title:r.get(4)?, album_title:r.get(5)?, album_artist:r.get(6)?, edition:r.get(7)?, disc:r.get(8)?, track:r.get(9)?, available:r.get(10)? }); }
            Ok(items)
        })
    }

    /// Creates one immutable reviewed selection. A concurrent regrouping rejects the review.
    pub fn create_scan(
        &self,
        groups: Vec<ReviewedGroup>,
        mode: ScanMode,
        method: PeakMethod,
    ) -> Result<JobId, StorageError> {
        if groups.is_empty() || groups.iter().any(|g| g.members.is_empty()) {
            return Err(StorageError::invalid("scan", "Select at least one track"));
        }
        self.write(move |tx| {
            let id = JobId::generate();
            tx.execute("INSERT INTO rg_scans(id,created_at_ms,analyzer,target_lufs,peak_method,mode,state) VALUES(?1,?2,?3,?4,?5,?6,'queued')",params![id.to_bytes(),now_ms(),ANALYZER,TARGET_LUFS,peak_sql(method),mode.sql()])?;
            for (ordinal,group) in groups.iter().enumerate() {
                tx.execute("INSERT INTO rg_groups(scan_id,ordinal,name,warning) VALUES(?1,?2,?3,?4)",params![id.to_bytes(),ordinal as i64,group.name,group.warning])?;
                for (album,expected) in group.albums.iter().filter(|_|mode != ScanMode::Tracks) {
                    let mut actual = tx.prepare("SELECT file_id FROM local_songs WHERE album_id=?1 ORDER BY file_id")?.query_map([album.to_bytes()],|r| r.get::<_,Vec<u8>>(0))?.collect::<Result<Vec<_>,_>>()?;
                    let mut expected = expected.iter().map(|f| f.to_bytes().to_vec()).collect::<Vec<_>>();
                    actual.sort(); expected.sort();
                    if actual != expected { return Err(StorageError::Conflict("Album membership changed; review the groups again".into())); }
                    tx.execute("INSERT INTO rg_group_albums VALUES(?1,?2,?3)", params![id.to_bytes(),ordinal as i64,album.to_bytes()])?;
                }
                for (index,member) in group.members.iter().enumerate() {
                    tx.execute("INSERT INTO rg_members(scan_id,group_ordinal,ordinal,file_id,title,state) VALUES(?1,?2,?3,?4,?5,'pending')",params![id.to_bytes(),ordinal as i64,index as i64,member.file.to_bytes(),member.title])?;
                }
            }
            Ok(id)
        })
    }

    pub fn cache_scan_measurement(
        &self,
        file: MediaFileId,
        signature: FileSignature,
        analysis: TrackAnalysis,
    ) -> Result<CachedAnalysis, StorageError> {
        if !analysis.valid() {
            return Err(StorageError::invalid(
                "measurement",
                "Non-finite or invalid result",
            ));
        }
        let sig = signature_to_sql(&signature)?;
        let digest = sig.digest.ok_or(StorageError::invalid(
            "measurement",
            "A full content digest is required",
        ))?;
        let energies = analysis
            .energies
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        self.write(move |tx| {
            tx.execute("INSERT INTO rg_measurements(file_id,digest,analyzer,peak_method,size_bytes,modified_unix_nanos,measured_at_ms,sample_rate,channel_mask,frames,sample_peak,peak,loudness_lufs,energies) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) ON CONFLICT(file_id,digest,analyzer,peak_method) DO NOTHING",params![file.to_bytes(),digest,ANALYZER,peak_sql(analysis.peak_method),sig.size_bytes,sig.modified_unix_nanos,now_ms(),analysis.sample_rate,analysis.channel_mask as i64,analysis.frames as i64,analysis.sample_peak,analysis.peak,analysis.loudness_lufs,energies])?;
            let id = tx.query_row("SELECT id FROM rg_measurements WHERE file_id=?1 AND digest=?2 AND analyzer=?3 AND peak_method=?4",params![file.to_bytes(),digest,ANALYZER,peak_sql(analysis.peak_method)],|r| r.get(0))?;
            Ok(CachedAnalysis { id,signature,analysis })
        })
    }
    pub fn cached_scan_measurement(
        &self,
        file: MediaFileId,
        signature: FileSignature,
        method: PeakMethod,
    ) -> Result<Option<CachedAnalysis>, StorageError> {
        let Some(digest) = signature.content_digest else {
            return Ok(None);
        };
        self.read(move |tx| {
            let mut stmt = tx.prepare(&format!("{MEASUREMENT_COLUMNS} WHERE file_id=?1 AND digest=?2 AND analyzer=?3 AND peak_method=?4"))?;
            let mut rows = stmt.query(params![file.to_bytes(),digest.0,ANALYZER,peak_sql(method)])?;
            rows.next()?.map(measurement).transpose()
        })
    }
    pub fn set_scan_state(
        &self,
        id: JobId,
        state: ScanState,
        error: Option<String>,
    ) -> Result<(), StorageError> {
        self.write(move |tx| {
            let from: String = tx.query_row(
                "SELECT state FROM rg_scans WHERE id=?1",
                [id.to_bytes()],
                |r| r.get(0),
            )?;
            let allowed = matches!(
                (ScanState::parse(&from)?, state),
                (
                    ScanState::Queued,
                    ScanState::Scanning | ScanState::Cancelled | ScanState::Failed
                ) | (
                    ScanState::Scanning,
                    ScanState::Ready
                        | ScanState::Incomplete
                        | ScanState::Failed
                        | ScanState::Cancelled
                        | ScanState::Stale
                )
            );
            if !allowed {
                return Err(StorageError::Conflict("Scan state changed".into()));
            }
            if state == ScanState::Ready {
                let pending: i64 = tx.query_row(
                    "SELECT count(*) FROM rg_members WHERE scan_id=?1 AND state<>'done'",
                    [id.to_bytes()],
                    |r| r.get(0),
                )?;
                let stale: i64 = tx.query_row(
                    "SELECT count(*) FROM rg_groups WHERE scan_id=?1 AND stale=1",
                    [id.to_bytes()],
                    |r| r.get(0),
                )?;
                if pending != 0 || stale != 0 {
                    return Err(StorageError::Conflict(
                        "The scan is incomplete or stale".into(),
                    ));
                }
            }
            tx.execute(
                "UPDATE rg_scans SET state=?2,error=?3 WHERE id=?1",
                params![id.to_bytes(), state.sql(), error],
            )?;
            Ok(())
        })
    }
    pub fn record_scan_member(
        &self,
        scan: JobId,
        file: MediaFileId,
        measurement: Option<i64>,
        detail: Option<String>,
        reused: bool,
    ) -> Result<(), StorageError> {
        self.write(move |tx| {
            if let Some(mid) = measurement {
                let compatible: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM rg_measurements m JOIN rg_scans s ON s.id=?1 WHERE m.id=?2 AND m.file_id=?3 AND m.analyzer=s.analyzer AND m.peak_method=s.peak_method)",params![scan.to_bytes(),mid,file.to_bytes()],|r|r.get(0))?;
                if !compatible {return Err(StorageError::Conflict("Measurement belongs to another file or scan configuration".into()));}
            }
            let updated=tx.execute("UPDATE rg_members SET state=?3,measurement_id=?4,detail=?5,reused=?6 WHERE scan_id=?1 AND file_id=?2 AND EXISTS(SELECT 1 FROM rg_scans WHERE id=?1 AND state='scanning')",params![scan.to_bytes(),file.to_bytes(),if measurement.is_some(){"done"}else{"failed"},measurement,detail,reused])?;
            if updated != 1 { return Err(StorageError::Conflict("Scan is no longer running".into())); } Ok(())
        })
    }
    pub fn finish_scan(&self, scan: JobId) -> Result<ScanState, StorageError> {
        self.write(move |tx| {
            let (pending, failed): (i64, i64) = tx.query_row(
                "SELECT count(*) FILTER(WHERE state='pending'),count(*) FILTER(WHERE state='failed') FROM rg_members WHERE scan_id=?1",
                [scan.to_bytes()], |r| Ok((r.get(0)?,r.get(1)?)),
            )?;
            if pending > 0 { return Err(StorageError::Conflict("The scan still has pending tracks".into())); }
            let stale: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM rg_groups WHERE scan_id=?1 AND stale=1)",[scan.to_bytes()],|r|r.get(0))?;
            let missing_album: bool = tx.query_row("SELECT mode<>'tracks' AND EXISTS(SELECT 1 FROM rg_groups WHERE scan_id=?1 AND peak IS NULL) FROM rg_scans WHERE id=?1",[scan.to_bytes()],|r|r.get(0))?;
            let state = if stale {ScanState::Stale} else if failed > 0 || missing_album {ScanState::Incomplete} else {ScanState::Ready};
            let changed = tx.execute("UPDATE rg_scans SET state=?2 WHERE id=?1 AND state='scanning'",params![scan.to_bytes(),state.sql()])?;
            if changed != 1 {return Err(StorageError::Conflict("Scan is no longer running".into()));}
            Ok(state)
        })
    }
    pub fn record_scan_album(
        &self,
        scan: JobId,
        ordinal: usize,
        result: Option<AlbumAnalysis>,
    ) -> Result<(), StorageError> {
        if result.as_ref().is_some_and(|r| !r.valid()) {
            return Err(StorageError::invalid(
                "album measurement",
                "Non-finite result",
            ));
        }
        self.write(move |tx| {
            tx.execute("UPDATE rg_groups SET gain_db=?3,peak=?4 WHERE scan_id=?1 AND ordinal=?2 AND stale=0 AND EXISTS(SELECT 1 FROM rg_scans WHERE id=?1 AND state='scanning') AND NOT EXISTS(SELECT 1 FROM rg_members WHERE scan_id=?1 AND group_ordinal=?2 AND state<>'done')",params![scan.to_bytes(),ordinal as i64,result.as_ref().and_then(AlbumAnalysis::gain_db),result.map(|r|r.peak)])?; Ok(())
        })
    }
    pub fn mark_scan_stale(&self, scan: JobId) -> Result<(), StorageError> {
        self.write(move |tx| {
            tx.execute(
                "UPDATE rg_groups SET stale=1 WHERE scan_id=?1",
                [scan.to_bytes()],
            )?;
            Ok(())
        })
    }
    /// Publication is separate from checkpoints and refuses every incomplete or invalidated group.
    pub fn publish_scan(&self, scan: JobId) -> Result<(), StorageError> {
        self.write(move |tx| {
            let changed=tx.execute("UPDATE rg_scans SET saved=1 WHERE id=?1 AND analyzer=?2 AND target_lufs=?3 AND state='ready' AND NOT EXISTS(SELECT 1 FROM rg_groups WHERE scan_id=?1 AND stale=1) AND NOT EXISTS(SELECT 1 FROM rg_members WHERE scan_id=?1 AND state<>'done') AND (mode='tracks' OR NOT EXISTS(SELECT 1 FROM rg_groups WHERE scan_id=?1 AND peak IS NULL))",params![scan.to_bytes(),ANALYZER,TARGET_LUFS])?;
            if changed!=1 { return Err(StorageError::Conflict("Rescan incomplete or stale results before saving".into())); }
            tx.execute("INSERT INTO rg_publications(file_id,scan_id) SELECT file_id,scan_id FROM rg_members WHERE scan_id=?1 ON CONFLICT(file_id) DO UPDATE SET scan_id=excluded.scan_id", [scan.to_bytes()])?;
            Ok(())
        })
    }
    /// Latest explicitly saved app result, including invalidation. A consumer must never treat
    /// stale values as current or fall back silently to an older saved selection.
    pub fn published_analysis(
        &self,
        file: MediaFileId,
    ) -> Result<Option<PublishedAnalysis>, StorageError> {
        self.read(move |tx| {
            let selected = tx
                .query_row(
                    "SELECT s.id,s.analyzer,s.target_lufs,m.measurement_id,g.gain_db,g.peak,g.stale
                 FROM rg_publications p JOIN rg_scans s ON s.id=p.scan_id
                 JOIN rg_members m ON m.scan_id=p.scan_id AND m.file_id=p.file_id
                 JOIN rg_groups g ON g.scan_id=m.scan_id AND g.ordinal=m.group_ordinal
                 WHERE p.file_id=?1",
                    [file.to_bytes()],
                    |r| {
                        Ok((
                            r.get::<_, Vec<u8>>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, f64>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, Option<f64>>(4)?,
                            r.get::<_, Option<f64>>(5)?,
                            r.get::<_, bool>(6)?,
                        ))
                    },
                )
                .optional()?;
            let Some((id, analyzer, target_lufs, mid, album_gain_db, album_peak, stale)) = selected
            else {
                return Ok(None);
            };
            let stale = stale || analyzer != ANALYZER || target_lufs != TARGET_LUFS;
            let mut stmt = tx.prepare(&format!("{MEASUREMENT_COLUMNS} WHERE id=?1"))?;
            let mut rows = stmt.query([mid])?;
            let track = measurement(rows.next()?.ok_or_else(invalid)?)?;
            Ok(Some(PublishedAnalysis {
                scan: JobId::from_bytes(id.try_into().map_err(|_| invalid())?),
                analyzer,
                target_lufs,
                track,
                album_gain_db,
                album_peak,
                stale,
            }))
        })
    }

    pub fn scan_history(&self) -> Result<Vec<ScanRecord>, StorageError> {
        self.read(|tx| {
            let mut stmt=tx.prepare("SELECT id,created_at_ms,analyzer,target_lufs,peak_method,mode,state,saved,error FROM rg_scans ORDER BY created_at_ms DESC,id DESC LIMIT 100")?;
            let mut rows=stmt.query([])?; let mut records=Vec::new();
            while let Some(r)=rows.next()? {
                let id=JobId::from_bytes(blob16(r,0,"rg_scans.id")?);
                let mut record=ScanRecord { id,created_at_ms:r.get(1)?,analyzer:r.get(2)?,target_lufs:r.get(3)?,method:peak_parse(&r.get::<_,String>(4)?)?,mode:ScanMode::parse(&r.get::<_,String>(5)?)?,state:ScanState::parse(&r.get::<_,String>(6)?)?,saved:r.get(7)?,error:r.get(8)?,groups:Vec::new() };
                let mut groups=tx.prepare("SELECT ordinal,name,warning,stale,gain_db,peak FROM rg_groups WHERE scan_id=?1 ORDER BY ordinal")?;
                let mut groups=groups.query([id.to_bytes()])?;
                while let Some(g)=groups.next()? {
                    let ordinal:i64=g.get(0)?;
                    let mut members=tx.prepare("SELECT file_id,title,state,detail,measurement_id,reused FROM rg_members WHERE scan_id=?1 AND group_ordinal=?2 ORDER BY ordinal")?;
                    let mut members=members.query(params![id.to_bytes(),ordinal])?;
                    let mut results=Vec::new();
                    while let Some(m)=members.next()? {
                        let cached=match m.get::<_,Option<i64>>(4)? { Some(mid)=>{let mut stmt=tx.prepare("SELECT id,size_bytes,modified_unix_nanos,digest,loudness_lufs,peak FROM rg_measurements WHERE id=?1")?; let mut rows=stmt.query([mid])?;rows.next()?.map(measurement_summary).transpose()?},None=>None };
                        results.push(MemberResult { file:MediaFileId::from_bytes(blob16(m,0,"file_id")?),title:m.get(1)?,done:m.get::<_,String>(2)?=="done",detail:m.get(3)?,measurement:cached,reused:m.get(5)? });
                    }
                    let mut albums=tx.prepare("SELECT album_id FROM rg_group_albums WHERE scan_id=?1 AND group_ordinal=?2 ORDER BY album_id")?;
                    let albums=albums.query_map(params![id.to_bytes(),ordinal],|r|r.get::<_,Vec<u8>>(0))?.collect::<Result<Vec<_>,_>>()?.into_iter().map(|b|Ok((ReleaseId::from_bytes(b.try_into().map_err(|_|invalid())?),Vec::new()))).collect::<Result<Vec<_>,StorageError>>()?;
                    record.groups.push(GroupResult { reviewed:ReviewedGroup { name:g.get(1)?,warning:g.get(2)?,members:results.iter().map(|m|ScanMember{file:m.file,title:m.title.clone()}).collect(),albums },stale:g.get::<_,bool>(3)? || record.analyzer != ANALYZER || record.target_lufs != TARGET_LUFS,gain_db:g.get(4)?,peak:g.get(5)?,members:results });
                }
                if record.analyzer != ANALYZER || record.target_lufs != TARGET_LUFS {
                    record.error.get_or_insert_with(|| "Analysis version or target changed; review and rescan before saving".into());
                }
                if record.groups.iter().any(|g|g.stale) && matches!(record.state,ScanState::Ready) { record.state=ScanState::Stale; }
                records.push(record);
            }
            Ok(records)
        })
    }
}
const MEASUREMENT_COLUMNS: &str = "SELECT id,size_bytes,modified_unix_nanos,digest,peak_method,sample_rate,channel_mask,frames,sample_peak,peak,loudness_lufs,energies FROM rg_measurements";
fn measurement_summary(r: &Row<'_>) -> Result<MeasurementSummary, StorageError> {
    let loudness_lufs: Option<f64> = r.get(4)?;
    let peak: f64 = r.get(5)?;
    if loudness_lufs.is_some_and(|value| !value.is_finite()) || !peak.is_finite() || peak < 0.0 {
        return Err(invalid());
    }
    Ok(MeasurementSummary {
        id: r.get(0)?,
        signature: signature_from_sql(r.get(1)?, r.get(2)?, r.get(3)?)?,
        loudness_lufs,
        peak,
    })
}

fn measurement(r: &Row<'_>) -> Result<CachedAnalysis, StorageError> {
    let bytes: Vec<u8> = r.get(11)?;
    if !bytes.len().is_multiple_of(8) {
        return Err(invalid());
    }
    let analysis = TrackAnalysis {
        peak_method: peak_parse(&r.get::<_, String>(4)?)?,
        sample_rate: r.get(5)?,
        channel_mask: r.get::<_, i64>(6)? as u64,
        frames: r.get::<_, i64>(7)? as u64,
        sample_peak: r.get(8)?,
        peak: r.get(9)?,
        loudness_lufs: r.get(10)?,
        energies: bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| f64::from_le_bytes(*b))
            .collect(),
    };
    if !analysis.valid() {
        return Err(invalid());
    }
    Ok(CachedAnalysis {
        id: r.get(0)?,
        signature: signature_from_sql(r.get(1)?, r.get(2)?, r.get(3)?)?,
        analysis,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use parabolica_domain::files::ContentDigest;

    #[test]
    fn history_is_independent_of_the_aggregation_payload() {
        let directory = tempfile::tempdir().unwrap();
        let storage = Storage::open(directory.path().join("app.db")).unwrap();
        let file = MediaFileId::generate();
        let signature = FileSignature {
            size_bytes: 100,
            modified_unix_nanos: 1_700_000_000_000_000_000,
            content_digest: Some(ContentDigest([7; 32])),
        };
        let analysis = TrackAnalysis {
            sample_rate: 48000,
            channel_mask: 3,
            frames: 19200,
            peak_method: PeakMethod::Sample,
            sample_peak: 0.4,
            peak: 0.4,
            loudness_lufs: Some(-20.0),
            energies: vec![10.0_f64.powf((-20.0 + 0.691) / 10.0)],
        };
        let cached = storage
            .cache_scan_measurement(file, signature.clone(), analysis)
            .unwrap();
        let scan = storage
            .create_scan(
                vec![ReviewedGroup {
                    name: "Track".into(),
                    warning: String::new(),
                    members: vec![ScanMember {
                        file,
                        title: "Track".into(),
                    }],
                    albums: Vec::new(),
                }],
                ScanMode::Tracks,
                PeakMethod::Sample,
            )
            .unwrap();
        storage
            .set_scan_state(scan, ScanState::Scanning, None)
            .unwrap();
        storage
            .record_scan_member(scan, file, Some(cached.id), None, false)
            .unwrap();
        storage.finish_scan(scan).unwrap();
        let complete = storage
            .cached_scan_measurement(file, signature.clone(), PeakMethod::Sample)
            .unwrap()
            .unwrap();
        assert_eq!(complete.analysis.energies.len(), 1);
        storage
            .write(move |tx| {
                tx.execute(
                    "UPDATE rg_measurements SET energies=X'00' WHERE id=?1",
                    [cached.id],
                )?;
                Ok(())
            })
            .unwrap();
        let history = storage.scan_history().unwrap();
        assert_eq!(history[0].state, ScanState::Ready);
        let measured = history[0].groups[0].members[0]
            .measurement
            .as_ref()
            .unwrap();
        assert_eq!(measured.signature, signature);
        assert_eq!(measured.id, cached.id);
        assert_eq!(measured.loudness_lufs, Some(-20.0));
        assert_eq!(measured.gain_db(), Some(2.0));
        assert_eq!(measured.peak, 0.4);
        assert!(
            storage
                .cached_scan_measurement(file, signature, PeakMethod::Sample)
                .is_err()
        );
    }
    fn ready_track_scan(storage: &Storage, files: &[MediaFileId]) -> JobId {
        let signature = FileSignature {
            size_bytes: 100,
            modified_unix_nanos: 1_700_000_000_000_000_000,
            content_digest: Some(ContentDigest([7; 32])),
        };
        let scan = storage
            .create_scan(
                files
                    .iter()
                    .map(|file| ReviewedGroup {
                        name: "Track".into(),
                        warning: String::new(),
                        members: vec![ScanMember {
                            file: *file,
                            title: "Track".into(),
                        }],
                        albums: Vec::new(),
                    })
                    .collect(),
                ScanMode::Tracks,
                PeakMethod::Sample,
            )
            .unwrap();
        storage
            .set_scan_state(scan, ScanState::Scanning, None)
            .unwrap();
        for file in files {
            let cached = storage
                .cache_scan_measurement(
                    *file,
                    signature.clone(),
                    TrackAnalysis {
                        sample_rate: 48000,
                        channel_mask: 3,
                        frames: 19200,
                        peak_method: PeakMethod::Sample,
                        sample_peak: 0.4,
                        peak: 0.4,
                        loudness_lufs: Some(-20.0),
                        energies: vec![10.0_f64.powf((-20.0 + 0.691) / 10.0)],
                    },
                )
                .unwrap();
            storage
                .record_scan_member(scan, *file, Some(cached.id), None, false)
                .unwrap();
        }
        storage.finish_scan(scan).unwrap();
        scan
    }

    #[test]
    fn obsolete_analyzer_preserves_history_but_requires_fresh_measurements_and_publication() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.db");
        let storage = Storage::open(&path).unwrap();
        let mono = MediaFileId::generate();
        let stereo = MediaFileId::generate();
        let saved = ready_track_scan(&storage, &[mono, stereo]);
        storage.publish_scan(saved).unwrap();
        let preview = ready_track_scan(&storage, &[mono, stereo]);
        let legacy =
            "parabolica-rg2/1;ebur128/0.1.10;bs1770-4;400ms/100ms;absolute=-70;relative=-10";
        storage
            .write(move |tx| {
                tx.execute("UPDATE rg_scans SET analyzer=?1", [legacy])?;
                tx.execute("UPDATE rg_measurements SET analyzer=?1", [legacy])?;
                tx.execute(
                    "UPDATE rg_measurements SET channel_mask=4 WHERE file_id=?1",
                    [mono.to_bytes()],
                )?;
                Ok(())
            })
            .unwrap();
        let signature = FileSignature {
            size_bytes: 100,
            modified_unix_nanos: 1_700_000_000_000_000_000,
            content_digest: Some(ContentDigest([7; 32])),
        };
        for file in [mono, stereo] {
            assert!(
                storage
                    .cached_scan_measurement(file, signature.clone(), PeakMethod::Sample)
                    .unwrap()
                    .is_none()
            );
            let published = storage.published_analysis(file).unwrap().unwrap();
            assert!(published.stale);
            assert_eq!(published.analyzer, legacy);
            assert_eq!(published.scan, saved);
            assert!(published.track.analysis.valid());
        }
        let history = storage.scan_history().unwrap();
        assert_eq!(history.len(), 2);
        for record in &history {
            assert_eq!(record.state, ScanState::Stale);
            assert_eq!(record.analyzer, legacy);
            assert_eq!(record.saved, record.id == saved);
            assert!(record.error.as_ref().unwrap().contains("review and rescan"));
            assert!(record.groups.iter().all(|g| g.stale));
            assert!(
                record
                    .groups
                    .iter()
                    .all(|g| g.members[0].measurement.is_some())
            );
        }
        assert!(storage.publish_scan(saved).is_err());
        assert!(storage.publish_scan(preview).is_err());
        let current = ready_track_scan(&storage, &[mono, stereo]);
        storage.publish_scan(current).unwrap();
        drop(storage);
        let reopened = Storage::open(&path).unwrap();
        assert_eq!(reopened.scan_history().unwrap().len(), 3);
        for file in [mono, stereo] {
            let published = reopened.published_analysis(file).unwrap().unwrap();
            assert!(!published.stale);
            assert_eq!(published.analyzer, ANALYZER);
            assert_eq!(published.scan, current);
            let cached = reopened
                .cached_scan_measurement(file, signature.clone(), PeakMethod::Sample)
                .unwrap()
                .unwrap();
            assert_eq!(cached.id, published.track.id);
        }
        let old = reopened.scan_history().unwrap();
        assert!(
            old.iter()
                .filter(|r| r.id != current)
                .all(|r| r.state == ScanState::Stale)
        );
    }
    #[test]
    fn latest_save_selects_each_file_and_survives_restart_without_stale_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.db");
        let storage = Storage::open(&path).unwrap();
        let first = MediaFileId::generate();
        let second = MediaFileId::generate();
        let older = ready_track_scan(&storage, &[first, second]);
        let newer = ready_track_scan(&storage, &[first]);
        storage
            .write(move |tx| {
                tx.execute(
                    "UPDATE rg_scans SET created_at_ms=100 WHERE id=?1",
                    [older.to_bytes()],
                )?;
                tx.execute(
                    "UPDATE rg_scans SET created_at_ms=200 WHERE id=?1",
                    [newer.to_bytes()],
                )?;
                Ok(())
            })
            .unwrap();
        storage.publish_scan(newer).unwrap();
        assert_eq!(
            storage.published_analysis(first).unwrap().unwrap().scan,
            newer
        );
        assert!(storage.published_analysis(second).unwrap().is_none());
        storage.publish_scan(older).unwrap();
        assert_eq!(
            storage.published_analysis(first).unwrap().unwrap().scan,
            older
        );
        assert_eq!(
            storage.published_analysis(second).unwrap().unwrap().scan,
            older
        );
        storage.publish_scan(newer).unwrap();
        assert_eq!(
            storage.published_analysis(first).unwrap().unwrap().scan,
            newer
        );
        assert_eq!(
            storage.published_analysis(second).unwrap().unwrap().scan,
            older
        );
        storage.mark_scan_stale(newer).unwrap();
        assert!(storage.publish_scan(newer).is_err());
        drop(storage);
        let reopened = Storage::open(&path).unwrap();
        let selected = reopened.published_analysis(first).unwrap().unwrap();
        assert_eq!(selected.scan, newer);
        assert!(selected.stale);
        assert_eq!(
            reopened.published_analysis(second).unwrap().unwrap().scan,
            older
        );
    }
    #[test]
    fn publication_migration_preserves_the_legacy_per_file_selection() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        let legacy = crate::migrate::MIGRATIONS
            .iter()
            .filter(|migration| migration.name != "0010_02_publication_selection.sql")
            .collect::<Vec<_>>();
        crate::migrate::apply(&mut connection, &legacy).unwrap();
        let first = MediaFileId::generate();
        let second = MediaFileId::generate();
        let older = JobId::generate();
        let newer = JobId::generate();
        let preview = JobId::generate();
        for (id, files, saved, created) in [
            (older, vec![first, second], true, 100),
            (newer, vec![first], true, 200),
            (preview, vec![first], false, 300),
        ] {
            connection.execute("INSERT INTO rg_scans(id,created_at_ms,analyzer,target_lufs,peak_method,mode,state,saved) VALUES(?1,?2,?3,-18,'sample','tracks','ready',?4)", params![id.to_bytes(), created, ANALYZER, saved]).unwrap();
            for (ordinal, file) in files.iter().enumerate() {
                connection.execute("INSERT INTO rg_groups(scan_id,ordinal,name,warning) VALUES(?1,?2,'Track','')", params![id.to_bytes(), ordinal as i64]).unwrap();
                connection.execute("INSERT OR IGNORE INTO rg_measurements(id,file_id,digest,analyzer,peak_method,size_bytes,modified_unix_nanos,measured_at_ms,sample_rate,channel_mask,frames,sample_peak,peak,loudness_lufs,energies) VALUES(?1,?2,zeroblob(32),?3,'sample',100,0,0,48000,3,19200,0,0,NULL,zeroblob(8))",params![if *file == first {1} else {2}, file.to_bytes(), ANALYZER]).unwrap();
                connection.execute("INSERT INTO rg_members(scan_id,group_ordinal,ordinal,file_id,title,state,measurement_id) VALUES(?1,?2,0,?3,'Track','done',?4)", params![id.to_bytes(), ordinal as i64, file.to_bytes(), if *file == first {1} else {2}]).unwrap();
            }
        }
        let migration = crate::migrate::MIGRATIONS
            .iter()
            .find(|migration| migration.name == "0010_02_publication_selection.sql")
            .unwrap();
        crate::migrate::apply(&mut connection, &[migration]).unwrap();
        for (file, expected) in [(first, newer), (second, older)] {
            let scan: Vec<u8> = connection
                .query_row(
                    "SELECT scan_id FROM rg_publications WHERE file_id=?1",
                    [file.to_bytes()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(scan, expected.to_bytes());
        }
        let count: i64 = connection
            .query_row("SELECT count(*) FROM rg_publications", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
