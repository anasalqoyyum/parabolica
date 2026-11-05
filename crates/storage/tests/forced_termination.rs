//! Kill a process that is writing checkpoints and inspect what it left behind.
//!
//! The test re-runs its own binary as the writer: `writer_process` does nothing unless
//! `PARABOLICA_STORAGE_WRITER_DB` is set.

mod common;

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use common::{file_record, queue_of, youtube};
use parabolica_domain::jobs::{JobKind, JobState, Progress};
use parabolica_domain::source::PlayableSource;
use parabolica_storage::Storage;
use parabolica_storage::jobs::{ItemOutcome, ItemReport, NewJobItem};

const DATABASE_VARIABLE: &str = "PARABOLICA_STORAGE_WRITER_DB";
const ROUNDS_BEFORE_KILL: u64 = 25;

#[test]
fn writer_process() {
    let Ok(path) = std::env::var(DATABASE_VARIABLE) else {
        return;
    };
    let storage = Storage::open(&path).unwrap();
    let root = storage.add_library_root(Path::new("/music"), None).unwrap();
    let items = (0..10_000)
        .map(|index| NewJobItem {
            key: index.to_string(),
            file: None,
        })
        .collect();
    let job = storage.create_job(JobKind::LibraryIndex, items).unwrap();
    storage
        .transition_job(
            job,
            JobState::Running(Progress {
                done: 0,
                total: Some(10_000),
            }),
        )
        .unwrap();
    for round in 0_u64..u64::MAX {
        let sources = (0..=round % 50)
            .map(|index| PlayableSource::Provider(youtube(&format!("{round}-{index}"))))
            .collect();
        storage
            .save_queue(queue_of(sources, Duration::from_millis(round)))
            .unwrap();
        storage
            .record_media_file(file_record(
                root,
                &format!("{round}.flac"),
                &[("TITLE", "t")],
            ))
            .unwrap();
        storage
            .record_job_items(
                job,
                vec![ItemReport {
                    key: round.to_string(),
                    outcome: ItemOutcome::Done,
                    detail: None,
                    signature: None,
                }],
            )
            .unwrap();
        // Printed only after every write of the round committed.
        println!("committed {round}");
    }
}

#[test]
fn killing_a_writer_leaves_a_consistent_database_with_its_last_commits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parabolica.db");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "writer_process",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(DATABASE_VARIABLE, &path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut last_committed = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        if let Some(round) = line.unwrap().strip_prefix("committed ") {
            let round: u64 = round.parse().unwrap();
            last_committed = Some(round);
            if round == ROUNDS_BEFORE_KILL {
                child.kill().unwrap();
                break;
            }
        }
    }
    child.wait().unwrap();
    let last_committed = last_committed.expect("the writer committed at least one round");

    // Read-only, so inspecting does not checkpoint or otherwise repair what the writer left.
    let conn =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
    drop(conn);

    let storage = Storage::open(&path).unwrap();
    // The writer may have committed more after its last report, never less.
    let queue = storage
        .load_queue()
        .unwrap()
        .expect("a checkpoint was saved");
    let round = u64::try_from(queue.position.as_millis()).unwrap();
    assert!(
        round >= last_committed,
        "checkpoint {round} predates reported {last_committed}"
    );
    assert_eq!(queue.entries.len() as u64, round % 50 + 1);

    let job = &storage.unfinished_jobs().unwrap()[0];
    let done = storage
        .job_items(job.id)
        .unwrap()
        .iter()
        .filter(|item| item.outcome == ItemOutcome::Done)
        .count() as u64;
    assert_eq!(
        job.state,
        JobState::Running(Progress {
            done,
            total: Some(10_000)
        })
    );
    assert!(done > last_committed);

    let indexed = storage.search_media_files("t", 100_000).unwrap().len() as u64;
    assert!(indexed > last_committed);
}
