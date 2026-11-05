use std::collections::HashSet;

use parabolica_domain::files::{ContentDigest, FileSignature};
use parabolica_domain::ids::{IdParseError, MediaFileId, QueueEntryId, ReleaseId, TrackId};
use parabolica_domain::queue::{InsertPosition, QueueError, QueueOrder};
use parabolica_domain::source::PlayableSource;

#[test]
fn ids_round_trip_through_text_and_storage_bytes() {
    let release = ReleaseId::generate();
    let text = release.to_string();
    assert!(text.starts_with("rel_"));
    assert_eq!(text.parse::<ReleaseId>(), Ok(release));
    assert_eq!(ReleaseId::from_bytes(release.to_bytes()), release);
}

#[test]
fn text_form_of_one_kind_is_rejected_as_another() {
    let track = TrackId::generate().to_string();
    assert_eq!(
        track.parse::<ReleaseId>(),
        Err(IdParseError::WrongPrefix { expected: "rel" })
    );
    assert_eq!(
        "rel_not-a-uuid".parse::<ReleaseId>(),
        Err(IdParseError::InvalidBody)
    );
    // Hyphenated UUIDs are a different text form and must not be accepted silently.
    assert_eq!(
        "rel_0190c2a4-3b5f-7000-8000-000000000000".parse::<ReleaseId>(),
        Err(IdParseError::InvalidBody)
    );
}

#[test]
fn same_titled_editions_get_distinct_release_ids() {
    let editions: HashSet<ReleaseId> = (0..1000).map(|_| ReleaseId::generate()).collect();
    assert_eq!(editions.len(), 1000);
}

#[test]
fn file_signature_detects_rewrite_with_same_size_and_mtime() {
    let recorded = FileSignature {
        size_bytes: 41_000_000,
        modified_unix_nanos: 1_700_000_000_000_000_000,
        content_digest: Some(ContentDigest([1; 32])),
    };
    let rewritten = FileSignature {
        content_digest: Some(ContentDigest([2; 32])),
        ..recorded.clone()
    };
    let touched = FileSignature {
        modified_unix_nanos: recorded.modified_unix_nanos + 1,
        ..recorded.clone()
    };
    let undigested = FileSignature {
        content_digest: None,
        ..recorded.clone()
    };

    assert!(recorded.matches_recorded(&recorded));
    assert!(!rewritten.matches_recorded(&recorded));
    assert!(!touched.matches_recorded(&recorded));
    assert!(undigested.matches_recorded(&recorded));
}

#[test]
fn duplicate_sources_become_distinct_queue_entries() {
    let file = PlayableSource::LocalFile(MediaFileId::generate());
    let mut queue = QueueOrder::default();
    let ids = queue.insert([file.clone(), file.clone()], InsertPosition::End);

    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    assert!(queue.entries().iter().all(|entry| entry.source == file));
}

#[test]
fn queue_entry_ids_survive_reordering_and_removal() {
    let mut queue = QueueOrder::default();
    let ids = queue.insert(
        (0..4).map(|_| PlayableSource::LocalFile(MediaFileId::generate())),
        InsertPosition::End,
    );
    queue.set_current(ids[1]).unwrap();

    queue.move_entry(ids[3], 0).unwrap();
    queue.remove(ids[2]).unwrap();
    let after_current = queue.insert(
        [PlayableSource::LocalFile(MediaFileId::generate())],
        InsertPosition::AfterCurrent,
    );

    let order: Vec<QueueEntryId> = queue.entries().iter().map(|entry| entry.id).collect();
    assert_eq!(order, [ids[3], ids[0], ids[1], after_current[0]]);
    assert_eq!(queue.current(), Some(ids[1]));
    assert_eq!(
        queue.move_entry(ids[2], 0),
        Err(QueueError::UnknownEntry(ids[2]))
    );
}

#[test]
fn removing_the_current_entry_leaves_nothing_current() {
    let mut queue = QueueOrder::default();
    let ids = queue.insert(
        [PlayableSource::LocalFile(MediaFileId::generate())],
        InsertPosition::End,
    );
    queue.set_current(ids[0]).unwrap();
    queue.remove(ids[0]).unwrap();
    assert_eq!(queue.current(), None);
}
