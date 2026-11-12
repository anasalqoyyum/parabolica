use std::path::PathBuf;

use parabolica_domain::files::{ContentDigest, FileSignature};
use parabolica_domain::ids::{ArtworkId, MediaFileId, ReleaseId};
use parabolica_domain::jobs::{JobState, Progress};
use parabolica_domain::tags::{
    ArtworkChange, ArtworkDestination, FieldChange, FieldEdit, PictureType, TagField, TagPatch,
};
use parabolica_storage::tag_writes::{
    NewTagWrite, TagSidecarState, TagTargetState, TagWriteSidecar, TagWriteTarget,
};
use parabolica_storage::{Storage, StorageError};

fn signature(byte: u8) -> FileSignature {
    FileSignature {
        size_bytes: 100 + u64::from(byte),
        modified_unix_nanos: 1_700_000_000_000_000_000 + i128::from(byte),
        content_digest: Some(ContentDigest([byte; 32])),
    }
}

fn target(path: &str, fields: Vec<FieldChange>, artwork: Vec<ArtworkChange>) -> TagWriteTarget {
    TagWriteTarget {
        patch: TagPatch {
            target: MediaFileId::generate(),
            reviewed: signature(1),
            fields,
            artwork,
        },
        path: PathBuf::from(path),
    }
}

#[test]
fn reviewed_patches_round_trip_and_outcomes_decide_the_job_state() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let album = ReleaseId::generate();
    let art = ArtworkId::generate();
    let a = target(
        "/music/a.flac",
        vec![
            FieldChange {
                field: TagField::Genre,
                edit: FieldEdit::Replace(vec!["Jazz".into(), "Soul".into()]),
            },
            FieldChange {
                field: TagField::Custom("MOOD".into()),
                edit: FieldEdit::FillIfEmpty(vec!["calm".into()]),
            },
            FieldChange {
                field: TagField::Date,
                edit: FieldEdit::Remove,
            },
        ],
        vec![ArtworkChange {
            artwork: art,
            picture_type: PictureType::FrontCover,
            destination: ArtworkDestination::Both,
        }],
    );
    let b = target("/music/b.flac", Vec::new(), Vec::new());
    let sidecar = TagWriteSidecar {
        directory: PathBuf::from("/music"),
        file_name: "cover.png".into(),
        picture_type: PictureType::FrontCover,
        artwork: art,
        replace: vec!["folder.jpg".into(), "Cover.JPG".into()],
    };
    let job = storage
        .create_tag_write(NewTagWrite {
            album,
            retry_of: None,
            targets: vec![a.clone(), b.clone()],
            images: vec![(art, PathBuf::from("/data/staged/art.png"))],
            sidecars: vec![sidecar.clone()],
        })
        .unwrap();
    let record = storage.tag_write(job).unwrap().unwrap();
    assert_eq!(record.state, JobState::Queued);
    assert_eq!(
        record.targets,
        vec![
            (a.clone(), TagTargetState::Pending),
            (b.clone(), TagTargetState::Pending)
        ]
    );
    assert_eq!(record.sidecars, vec![(sidecar, TagSidecarState::Pending)]);
    assert_eq!(storage.unfinished_tag_writes().unwrap().len(), 1);

    // Outcomes are only accepted while the job runs.
    assert!(matches!(
        storage.record_tag_target(job, a.patch.target, TagTargetState::Unchanged),
        Err(StorageError::Conflict(_))
    ));
    storage.start_tag_write(job).unwrap();
    let backup = PathBuf::from("/data/backups/a.flac");
    storage
        .record_tag_target(
            job,
            a.patch.target,
            TagTargetState::Replacing {
                backup: backup.clone(),
                expected: signature(2),
            },
        )
        .unwrap();
    storage
        .record_tag_target(
            job,
            a.patch.target,
            TagTargetState::Written {
                signature: signature(2),
                backup: backup.clone(),
            },
        )
        .unwrap();
    storage
        .record_tag_target(
            job,
            b.patch.target,
            TagTargetState::Deferred {
                reason: "Playing in Parabolica".into(),
            },
        )
        .unwrap();
    storage
        .record_tag_sidecar(job, 0, TagSidecarState::Written)
        .unwrap();
    let running = storage.tag_write(job).unwrap().unwrap();
    assert_eq!(
        running.state,
        JobState::Running(Progress {
            done: 3,
            total: Some(3)
        })
    );
    assert_eq!(
        storage.finish_tag_write(job, false).unwrap(),
        JobState::Incomplete
    );
    assert!(storage.unfinished_tag_writes().unwrap().is_empty());

    storage
        .record_tag_restore(job, a.patch.target, signature(1))
        .unwrap();
    assert!(matches!(
        storage.record_tag_restore(job, b.patch.target, signature(1)),
        Err(StorageError::Conflict(_))
    ));
    storage.discard_tag_backups(job).unwrap();
    let finished = storage.tag_writes(album).unwrap();
    assert_eq!(finished.len(), 1);
    assert!(finished[0].backups_discarded);
    assert_eq!(
        finished[0].targets[0].1,
        TagTargetState::Restored {
            signature: signature(1)
        }
    );

    // A retry that writes everything completes; one that writes nothing fails.
    let retry = storage
        .create_tag_write(NewTagWrite {
            album,
            retry_of: Some(job),
            targets: vec![b.clone()],
            images: Vec::new(),
            sidecars: Vec::new(),
        })
        .unwrap();
    storage.start_tag_write(retry).unwrap();
    storage
        .record_tag_target(retry, b.patch.target, TagTargetState::Unchanged)
        .unwrap();
    assert_eq!(
        storage.finish_tag_write(retry, false).unwrap(),
        JobState::Completed
    );
    let failed = storage
        .create_tag_write(NewTagWrite {
            album,
            retry_of: Some(job),
            targets: vec![b.clone()],
            images: Vec::new(),
            sidecars: Vec::new(),
        })
        .unwrap();
    storage.start_tag_write(failed).unwrap();
    storage
        .record_tag_target(failed, b.patch.target, TagTargetState::Changed)
        .unwrap();
    assert!(matches!(
        storage.finish_tag_write(failed, false).unwrap(),
        JobState::Failed { .. }
    ));
    let history = storage.tag_writes(album).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].id, failed);
    assert_eq!(history[1].retry_of, Some(job));

    // Restart keeps the record.
    drop(storage);
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    assert_eq!(storage.tag_writes(album).unwrap(), history);
}

#[test]
fn invalid_writes_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("app.db")).unwrap();
    let album = ReleaseId::generate();
    let new = |targets| NewTagWrite {
        album,
        retry_of: None,
        targets,
        images: Vec::new(),
        sidecars: Vec::new(),
    };
    assert!(storage.create_tag_write(new(Vec::new())).is_err());
    let a = target("/music/a.flac", Vec::new(), Vec::new());
    assert!(
        storage
            .create_tag_write(new(vec![a.clone(), a.clone()]))
            .is_err()
    );
    let mut undigested = a;
    undigested.patch.reviewed.content_digest = None;
    assert!(storage.create_tag_write(new(vec![undigested])).is_err());
    // An artwork change must reference a staged image of the same job.
    let missing = target(
        "/music/b.flac",
        Vec::new(),
        vec![ArtworkChange {
            artwork: ArtworkId::generate(),
            picture_type: PictureType::BackCover,
            destination: ArtworkDestination::Embed,
        }],
    );
    assert!(storage.create_tag_write(new(vec![missing])).is_err());
    assert!(storage.tag_writes(album).unwrap().is_empty());
}
