use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

use parabolica_domain::control::CancelToken;
use parabolica_domain::files::FileSignature;
use parabolica_domain::ids::{ArtworkId, JobId, MediaFileId};
use parabolica_domain::tags::{
    ArtworkChange, ArtworkDestination, FieldChange, FieldEdit, PictureType, TagField, TagPatch,
};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::artwork::StagedImage;
use crate::flac::{self, Block, Picture, VorbisComments};
use crate::job::{self, Ledger, SidecarState, TargetPlan, TargetState};
use crate::write::{self, FileEdit, Hooks, NoHooks, Preparation, Step, WriteError};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../audio/tests/fixtures/flac/s16_44100.flac"
);

/// The audio fixture with a comment block, an application block, an icon picture, a front cover
/// and padding added, so preservation of each can be checked.
fn rich_flac(dir: &Path, name: &str) -> PathBuf {
    let bytes = fs::read(FIXTURE).unwrap();
    let metadata = flac::read_metadata(&mut Cursor::new(&bytes)).unwrap();
    let audio = &bytes[metadata.audio_offset as usize..];
    let mut blocks: Vec<Block> = metadata
        .blocks
        .into_iter()
        .filter(|b| b.kind != flac::VORBIS_COMMENT && b.kind != flac::PICTURE && b.kind != 1)
        .collect();
    blocks.push(Block {
        kind: flac::VORBIS_COMMENT,
        data: VorbisComments {
            vendor: b"reference libFLAC 1.4.3".to_vec(),
            entries: vec![
                b"TITLE=Old title".to_vec(),
                b"ALBUM=Old album".to_vec(),
                b"REPLAYGAIN_TRACK_GAIN=-6.50 dB".to_vec(),
                b"REPLAYGAIN_TRACK_PEAK=0.988".to_vec(),
                b"MY_CUSTOM=\xffkeep".to_vec(),
            ],
        }
        .encode(),
    });
    blocks.push(Block {
        kind: 2,
        data: b"TEST application payload".to_vec(),
    });
    for code in [1, 3] {
        blocks.push(Block {
            kind: flac::PICTURE,
            data: Picture {
                code,
                mime: "image/png".into(),
                description: format!("existing {code}"),
                width: 1,
                height: 1,
                depth: 24,
                colors: 0,
                data: vec![code as u8; 10],
            }
            .encode(),
        });
    }
    blocks.push(Block {
        kind: 1,
        data: vec![0; 1024],
    });
    let mut out = flac::encode_metadata(&blocks).unwrap();
    out.extend_from_slice(audio);
    let path = dir.join(name);
    fs::write(&path, out).unwrap();
    path
}

fn png(width: u32, height: u32, shade: u8) -> StagedImage {
    let mut out = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(width, height, image::Rgb([shade, 10, 10]))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    StagedImage::decode(out.into_inner()).unwrap()
}

fn decoded_pcm(path: &Path) -> Vec<i32> {
    let file = File::open(path).unwrap();
    let mut hint = Hint::new();
    hint.with_extension("flac");
    let mut reader = symphonia::default::get_probe()
        .probe(
            &hint,
            MediaSourceStream::new(Box::new(file), Default::default()),
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .unwrap();
    let track = reader.default_track(TrackType::Audio).unwrap();
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(
            track.codec_params.as_ref().unwrap().audio().unwrap(),
            &Default::default(),
        )
        .unwrap();
    let mut samples = Vec::new();
    while let Ok(Some(packet)) = reader.next_packet() {
        if packet.track_id != id {
            continue;
        }
        let buffer = decoder.decode(&packet).unwrap();
        let mut interleaved = vec![0i32; buffer.samples_interleaved()];
        buffer.copy_to_slice_interleaved(&mut interleaved);
        samples.extend(interleaved);
    }
    assert!(!samples.is_empty());
    samples
}

fn audio_bytes(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).unwrap();
    let metadata = flac::read_metadata(&mut Cursor::new(&bytes)).unwrap();
    bytes[metadata.audio_offset as usize..].to_vec()
}

fn blocks(path: &Path) -> Vec<Block> {
    flac::read_metadata(&mut File::open(path).unwrap())
        .unwrap()
        .blocks
}

fn replace(field: TagField, value: &str) -> FieldChange {
    FieldChange {
        field,
        edit: FieldEdit::Replace(vec![value.into()]),
    }
}

fn prepare(
    path: &Path,
    reviewed: &FileSignature,
    fields: &[FieldChange],
    pictures: &[(PictureType, &StagedImage)],
    hooks: &dyn Hooks,
) -> Result<Preparation, WriteError> {
    write::prepare(
        path,
        reviewed,
        FileEdit { fields, pictures },
        &job::temp_path(path, JobId::from_bytes([1; 16])),
        &path.with_extension("backup"),
        hooks,
    )
}

fn leftovers(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".parabolica-tmp") || name.ends_with(".backup"))
        .collect()
}

#[test]
fn write_changes_selected_fields_and_cover_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = rich_flac(dir.path(), "track.flac");
    let before_blocks = blocks(&path);
    let before_pcm = decoded_pcm(&path);
    let before_audio = audio_bytes(&path);
    let reviewed = write::fingerprint(&path).unwrap();
    let cover = png(4, 4, 1);

    let Preparation::Ready(prepared) = prepare(
        &path,
        &reviewed,
        &[
            replace(TagField::Title, "New title"),
            FieldChange {
                field: TagField::Genre,
                edit: FieldEdit::Replace(vec!["Jazz".into(), "Soul".into()]),
            },
        ],
        &[(PictureType::FrontCover, &cover)],
        &NoHooks,
    )
    .unwrap() else {
        panic!("expected a write");
    };
    let backup = prepared.backup().to_owned();
    let written = prepared.commit(&NoHooks).unwrap();
    assert_eq!(written, write::fingerprint(&path).unwrap());
    assert_eq!(fs::read(&backup).unwrap(), {
        let mut original = flac::encode_metadata(&before_blocks).unwrap();
        original.extend_from_slice(&before_audio);
        original
    });

    let tags = crate::write::read_tags(&path).unwrap();
    assert_eq!(tags.values(&TagField::Title), vec!["New title".to_owned()]);
    assert_eq!(
        tags.values(&TagField::Genre),
        vec!["Jazz".to_owned(), "Soul".to_owned()]
    );
    assert_eq!(tags.values(&TagField::Album), vec!["Old album".to_owned()]);
    assert_eq!(
        tags.values(&TagField::ReplayGainTrackGain),
        vec!["-6.50 dB".to_owned()]
    );
    let after = blocks(&path);
    let comments = VorbisComments::decode(
        &after
            .iter()
            .find(|b| b.kind == flac::VORBIS_COMMENT)
            .unwrap()
            .data,
    )
    .unwrap();
    assert_eq!(comments.vendor, b"reference libFLAC 1.4.3");
    assert!(comments.entries.contains(&b"MY_CUSTOM=\xffkeep".to_vec()));
    // Every block other than the comments and the front cover is byte-identical and in order.
    let untouched = |blocks: &[Block]| -> Vec<Block> {
        blocks
            .iter()
            .filter(|b| {
                b.kind != flac::VORBIS_COMMENT
                    && !(b.kind == flac::PICTURE && Picture::decode(&b.data).unwrap().code == 3)
            })
            .cloned()
            .collect()
    };
    assert_eq!(untouched(&after), untouched(&before_blocks));
    let covers: Vec<Picture> = after
        .iter()
        .filter(|b| b.kind == flac::PICTURE)
        .map(|b| Picture::decode(&b.data).unwrap())
        .filter(|p| p.code == 3)
        .collect();
    assert_eq!(covers.len(), 1);
    assert_eq!(covers[0].data, cover.bytes());
    assert_eq!((covers[0].width, covers[0].height), (4, 4));
    assert_eq!(audio_bytes(&path), before_audio);
    assert_eq!(decoded_pcm(&path), before_pcm);
    fs::remove_file(&backup).unwrap();
    assert!(leftovers(dir.path()).is_empty());

    // Repeating the same edit is a no-op: no rewrite and no duplicate picture.
    let again = write::fingerprint(&path).unwrap();
    assert!(matches!(
        prepare(
            &path,
            &again,
            &[replace(TagField::Title, "New title")],
            &[(PictureType::FrontCover, &cover)],
            &NoHooks,
        ),
        Ok(Preparation::Unchanged)
    ));
    assert_eq!(write::fingerprint(&path).unwrap(), again);
}

struct FailAt(Step, io::ErrorKind);

impl Hooks for FailAt {
    fn at(&self, step: Step, _: &Path) -> io::Result<()> {
        if step == self.0 {
            Err(io::Error::from(self.1))
        } else {
            Ok(())
        }
    }
}

#[test]
fn injected_failures_leave_the_original_and_no_temporary_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = rich_flac(dir.path(), "track.flac");
    let original = fs::read(&path).unwrap();
    let reviewed = write::fingerprint(&path).unwrap();
    for step in [Step::TempWritten, Step::BackupWritten, Step::BeforeReplace] {
        let hooks = FailAt(step, io::ErrorKind::StorageFull);
        let result = prepare(
            &path,
            &reviewed,
            &[replace(TagField::Title, "x")],
            &[],
            &hooks,
        )
        .and_then(|prepared| match prepared {
            Preparation::Ready(prepared) => prepared.commit(&hooks),
            Preparation::Unchanged => panic!("expected a write"),
        });
        assert_eq!(
            result,
            Err(WriteError::Failed("Disk is full".into())),
            "{step:?}"
        );
        assert_eq!(fs::read(&path).unwrap(), original, "{step:?}");
        assert!(leftovers(dir.path()).is_empty(), "{step:?}");
    }
}

/// Simulates another tagger saving the file just before the rename.
struct ConcurrentEdit;

impl Hooks for ConcurrentEdit {
    fn at(&self, step: Step, target: &Path) -> io::Result<()> {
        if step == Step::BeforeReplace {
            let mut bytes = fs::read(target)?;
            bytes.push(0);
            fs::write(target, bytes)?;
        }
        Ok(())
    }
}

#[test]
fn concurrent_and_same_size_edits_are_detected() {
    let dir = tempfile::tempdir().unwrap();
    let path = rich_flac(dir.path(), "track.flac");
    let reviewed = write::fingerprint(&path).unwrap();

    let Ok(Preparation::Ready(prepared)) = prepare(
        &path,
        &reviewed,
        &[replace(TagField::Title, "x")],
        &[],
        &ConcurrentEdit,
    ) else {
        panic!("expected a write");
    };
    let edited = fs::read(&path).unwrap();
    assert_eq!(prepared.commit(&ConcurrentEdit), Err(WriteError::Changed));
    assert_eq!(fs::read(&path).unwrap().len(), edited.len() + 1);

    // A same-size edit that restores the modification time is caught by the content digest.
    let path = rich_flac(dir.path(), "same-size.flac");
    let reviewed = write::fingerprint(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let mut bytes = fs::read(&path).unwrap();
    let at = bytes.windows(9).position(|w| w == b"Old title").unwrap();
    bytes[at] = b'X';
    fs::write(&path, &bytes).unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert!(matches!(
        prepare(
            &path,
            &reviewed,
            &[replace(TagField::Title, "y")],
            &[],
            &NoHooks
        ),
        Err(WriteError::Changed)
    ));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(leftovers(dir.path()).is_empty());
}

#[test]
fn read_only_and_non_flac_files_fail_without_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = rich_flac(dir.path(), "track.flac");
    let reviewed = write::fingerprint(&path).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions.clone()).unwrap();
    assert_eq!(
        prepare(
            &path,
            &reviewed,
            &[replace(TagField::Title, "x")],
            &[],
            &NoHooks
        )
        .err(),
        Some(WriteError::Failed("File is read-only".into()))
    );
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();

    let text = dir.path().join("notes.flac");
    fs::write(&text, b"not audio").unwrap();
    let reviewed = write::fingerprint(&text).unwrap();
    assert_eq!(
        prepare(
            &text,
            &reviewed,
            &[replace(TagField::Title, "x")],
            &[],
            &NoHooks
        )
        .err(),
        Some(WriteError::Failed("Not a FLAC file".into()))
    );
    assert_eq!(fs::read(&text).unwrap(), b"not audio");
}

#[derive(Default)]
struct MemoryLedger {
    targets: Vec<(MediaFileId, TargetState)>,
    sidecars: Vec<(usize, SidecarState)>,
    fail_on_written: bool,
}

impl Ledger for MemoryLedger {
    fn target(&mut self, file: MediaFileId, state: &TargetState) -> Result<(), String> {
        if self.fail_on_written && matches!(state, TargetState::Written { .. }) {
            return Err("database unavailable".into());
        }
        self.targets.push((file, state.clone()));
        Ok(())
    }

    fn sidecar(&mut self, index: usize, state: &SidecarState) -> Result<(), String> {
        self.sidecars.push((index, state.clone()));
        Ok(())
    }
}

fn plan(path: &Path, artwork: Vec<ArtworkChange>) -> TargetPlan {
    TargetPlan {
        patch: TagPatch {
            target: MediaFileId::generate(),
            reviewed: write::fingerprint(path).unwrap(),
            fields: vec![replace(TagField::Album, "New album")],
            artwork,
        },
        path: path.to_owned(),
    }
}

#[test]
fn job_records_outcomes_defers_playing_files_and_writes_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let disc = dir.path().join("Disc 1");
    fs::create_dir(&disc).unwrap();
    let a = rich_flac(&disc, "01.flac");
    let b = rich_flac(&disc, "02.flac");
    fs::write(disc.join("folder.jpg"), b"old cover").unwrap();
    let cover = png(8, 8, 2);
    let art = ArtworkId::generate();
    let images = HashMap::from([(art, cover.clone())]);
    let change = ArtworkChange {
        artwork: art,
        picture_type: PictureType::FrontCover,
        destination: ArtworkDestination::Both,
    };
    let targets = vec![plan(&a, vec![change.clone()]), plan(&b, vec![change])];
    let sidecars = job::plan_sidecars(&targets, &images, |dir, kind| {
        crate::artwork::existing_sidecars(dir, kind).unwrap()
    });
    assert_eq!(sidecars.len(), 1);
    assert_eq!(sidecars[0].file_name, "cover.png");
    assert_eq!(sidecars[0].replace, vec!["folder.jpg".to_owned()]);
    let backups = dir.path().join("backups");
    let id = JobId::generate();
    let mut ledger = MemoryLedger::default();
    let b_before = fs::read(&b).unwrap();
    let completed = job::run(
        job::Job {
            id,
            targets: &targets,
            sidecars: &sidecars,
            images: &images,
            backup_dir: &backups,
        },
        &mut ledger,
        &|path| path == b,
        &CancelToken::new(),
        &NoHooks,
    )
    .unwrap();
    assert!(completed);
    assert!(matches!(ledger.targets[0].1, TargetState::Replacing { .. }));
    let TargetState::Written { signature, backup } = &ledger.targets[1].1 else {
        panic!("{:?}", ledger.targets);
    };
    assert_eq!(*signature, write::fingerprint(&a).unwrap());
    assert!(backup.starts_with(job::job_backup_dir(&backups, id)));
    assert_eq!(
        ledger.targets[2].1,
        TargetState::Deferred {
            reason: "Playing in Parabolica".into()
        }
    );
    assert_eq!(fs::read(&b).unwrap(), b_before);
    assert_eq!(ledger.sidecars, vec![(0, SidecarState::Written)]);
    assert_eq!(fs::read(disc.join("cover.png")).unwrap(), cover.bytes());
    assert!(!disc.join("folder.jpg").exists());
    assert_eq!(
        fs::read(job::job_backup_dir(&backups, id).join("sidecars/0/folder.jpg")).unwrap(),
        b"old cover"
    );

    // Restoring copies the backup back while the file still has the written content.
    let restored = write::restore(&a, backup, signature, &a.with_extension("restore")).unwrap();
    assert_eq!(fs::read(&a).unwrap(), fs::read(backup).unwrap());
    assert_eq!(restored, write::fingerprint(&a).unwrap());
    assert_eq!(
        write::restore(&a, backup, signature, &a.with_extension("restore")),
        Err(WriteError::Changed)
    );

    // Replacing a sidecar whose name differs only in case keeps the new file.
    let other = dir.path().join("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("Cover.PNG"), b"old").unwrap();
    write::write_sidecar(
        &other,
        "cover.png",
        PictureType::FrontCover,
        &cover,
        &["Cover.PNG".to_owned()],
        &backups.join("case"),
        "case",
    )
    .unwrap();
    let names: Vec<_> = fs::read_dir(&other)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["cover.png"]);
    assert_eq!(fs::read(other.join("cover.png")).unwrap(), cover.bytes());
    assert_eq!(fs::read(backups.join("case/Cover.PNG")).unwrap(), b"old");

    // A sidecar that appeared after review is not overwritten.
    fs::write(disc.join("front.jpg"), b"new").unwrap();
    let mut ledger = MemoryLedger::default();
    job::run(
        job::Job {
            id: JobId::generate(),
            targets: &[],
            sidecars: &sidecars,
            images: &images,
            backup_dir: &backups,
        },
        &mut ledger,
        &|_| false,
        &CancelToken::new(),
        &NoHooks,
    )
    .unwrap();
    assert_eq!(ledger.sidecars, vec![(0, SidecarState::Changed)]);
    assert_eq!(fs::read(disc.join("front.jpg")).unwrap(), b"new");
    assert!(leftovers(&disc).is_empty());
}

#[test]
fn interrupted_targets_resolve_from_file_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = rich_flac(dir.path(), "track.flac");
    let targets = vec![plan(&path, Vec::new())];
    let reviewed = targets[0].patch.reviewed.clone();
    let backups = dir.path().join("backups");
    let id = JobId::generate();
    // The ledger fails after the rename, as a crash would.
    let mut ledger = MemoryLedger {
        fail_on_written: true,
        ..Default::default()
    };
    let result = job::run(
        job::Job {
            id,
            targets: &targets,
            sidecars: &[],
            images: &HashMap::new(),
            backup_dir: &backups,
        },
        &mut ledger,
        &|_| false,
        &CancelToken::new(),
        &NoHooks,
    );
    assert!(result.is_err());
    let replacing = ledger.targets.last().unwrap().1.clone();
    let TargetState::Replacing { backup, .. } = &replacing else {
        panic!("{replacing:?}");
    };
    assert!(matches!(
        job::recover(id, &path, &reviewed, &replacing),
        TargetState::Written { .. }
    ));

    // Replacing recorded but the rename never happened.
    fs::copy(backup, &path).unwrap();
    let original = fs::read(&path).unwrap();
    fs::write(job::temp_path(&path, id), b"partial").unwrap();
    assert_eq!(
        job::recover(id, &path, &reviewed, &replacing),
        TargetState::Failed {
            reason: "Interrupted before replacement; the file is unchanged".into()
        }
    );
    assert!(!job::temp_path(&path, id).exists());
    assert!(!backup.exists());
    assert_eq!(fs::read(&path).unwrap(), original);

    // Edited by something else since.
    fs::write(&path, b"other").unwrap();
    assert_eq!(
        job::recover(id, &path, &reviewed, &replacing),
        TargetState::Changed
    );
    assert!(matches!(
        job::recover(id, &path, &reviewed, &TargetState::Pending),
        TargetState::Failed { .. }
    ));
}

#[test]
fn cancellation_stops_between_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = rich_flac(dir.path(), "track.flac");
    let targets = vec![plan(&path, Vec::new())];
    let cancel = CancelToken::new();
    cancel.cancel();
    let mut ledger = MemoryLedger::default();
    let completed = job::run(
        job::Job {
            id: JobId::generate(),
            targets: &targets,
            sidecars: &[],
            images: &HashMap::new(),
            backup_dir: dir.path(),
        },
        &mut ledger,
        &|_| false,
        &cancel,
        &NoHooks,
    )
    .unwrap();
    assert!(!completed);
    assert!(ledger.targets.is_empty());
}

#[test]
fn missing_comment_block_is_added_and_files_without_one_read_empty() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = fs::read(FIXTURE).unwrap();
    let metadata = flac::read_metadata(&mut Cursor::new(&bytes)).unwrap();
    let streaminfo: Vec<Block> = metadata
        .blocks
        .into_iter()
        .filter(|b| b.kind == flac::STREAMINFO)
        .collect();
    let mut out = flac::encode_metadata(&streaminfo).unwrap();
    out.extend_from_slice(&bytes[metadata.audio_offset as usize..]);
    let path = dir.path().join("bare.flac");
    fs::write(&path, out).unwrap();
    assert!(write::read_tags(&path).unwrap().comments().is_empty());
    let reviewed = write::fingerprint(&path).unwrap();
    let Ok(Preparation::Ready(prepared)) = prepare(
        &path,
        &reviewed,
        &[replace(TagField::Title, "Added")],
        &[],
        &NoHooks,
    ) else {
        panic!("expected a write");
    };
    prepared.commit(&NoHooks).unwrap();
    let mut header = [0; 4];
    File::open(&path).unwrap().read_exact(&mut header).unwrap();
    assert_eq!(&header, b"fLaC");
    assert_eq!(
        write::read_tags(&path).unwrap().comments(),
        vec![("TITLE".to_owned(), "Added".to_owned())]
    );
    assert_eq!(blocks(&path)[0].kind, flac::STREAMINFO);
}
