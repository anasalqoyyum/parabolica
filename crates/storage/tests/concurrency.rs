//! Concurrent workers and readers through one storage handle.

mod common;

use std::sync::Barrier;
use std::thread;
use std::time::Duration;

use common::{AppearanceMode, queue_of, youtube};
use parabolica_domain::jobs::{JobKind, JobState, Progress};
use parabolica_domain::source::PlayableSource;
use parabolica_storage::jobs::{ItemOutcome, ItemReport, NewJobItem};
use parabolica_storage::{Storage, StorageError};

const ITEMS: usize = 200;

#[test]
fn concurrent_workers_readers_and_a_cancel_keep_every_invariant() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("parabolica.db")).unwrap();

    let jobs: Vec<_> = (0..4)
        .map(|_| {
            let items = (0..ITEMS)
                .map(|index| NewJobItem {
                    key: format!("item-{index}"),
                    file: None,
                })
                .collect();
            let job = storage.create_job(JobKind::ReplayGainScan, items).unwrap();
            storage
                .transition_job(
                    job,
                    JobState::Running(Progress {
                        done: 0,
                        total: Some(ITEMS as u64),
                    }),
                )
                .unwrap();
            job
        })
        .collect();
    let cancelled = jobs[0];
    let start = Barrier::new(jobs.len() + 3);

    thread::scope(|scope| {
        for &job in &jobs {
            let storage = storage.clone();
            let start = &start;
            scope.spawn(move || {
                start.wait();
                let mut rejected = false;
                for index in 0..ITEMS {
                    let report = ItemReport {
                        key: format!("item-{index}"),
                        outcome: ItemOutcome::Done,
                        detail: None,
                        signature: None,
                    };
                    match storage.record_job_items(job, vec![report]) {
                        Ok(()) => assert!(!rejected, "a report was accepted after a rejection"),
                        Err(StorageError::Conflict(_)) if job == cancelled => rejected = true,
                        Err(error) => panic!("{error}"),
                    }
                }
                if !rejected {
                    // The cancel may land before or after this; exactly one side wins.
                    match storage.transition_job(job, JobState::Completed) {
                        Ok(()) | Err(StorageError::JobTransition { .. }) => {}
                        Err(error) => panic!("{error}"),
                    }
                }
            });
        }

        let canceller = storage.clone();
        let start_ref = &start;
        scope.spawn(move || {
            start_ref.wait();
            match canceller.transition_job(cancelled, JobState::Cancelled) {
                Ok(()) | Err(StorageError::JobTransition { .. }) => {}
                Err(error) => panic!("{error}"),
            }
        });

        let checkpointer = storage.clone();
        scope.spawn(move || {
            start_ref.wait();
            for round in 0..30 {
                let sources = (0..=round % 7)
                    .map(|index| PlayableSource::Provider(youtube(&format!("{round}-{index}"))))
                    .collect();
                checkpointer
                    .save_queue(queue_of(sources, Duration::from_millis(round)))
                    .unwrap();
                checkpointer.save_preference(&AppearanceMode::Dark).unwrap();
            }
        });

        let reader = storage.clone();
        scope.spawn(move || {
            start_ref.wait();
            for _ in 0..200 {
                // `load_queue` validates the checkpoint, so a torn read would surface as an error.
                if let Some(queue) = reader.load_queue().unwrap() {
                    let round = queue.position.as_millis() as usize;
                    assert_eq!(queue.entries.len(), round % 7 + 1);
                }
                for job in reader.unfinished_jobs().unwrap() {
                    let JobState::Running(progress) = job.state else {
                        panic!("unfinished job in state {:?}", job.state);
                    };
                    assert!(progress.done <= ITEMS as u64);
                }
            }
        });
    });

    for &job in &jobs {
        let record = storage.job(job).unwrap().unwrap();
        let items = storage.job_items(job).unwrap();
        let done = items
            .iter()
            .filter(|item| item.outcome == ItemOutcome::Done)
            .count() as u64;
        assert_eq!(
            record.progress.done, done,
            "progress matches recorded items"
        );
        match record.state {
            JobState::Completed => assert_eq!(done, ITEMS as u64),
            JobState::Cancelled => assert_eq!(job, cancelled),
            other => panic!("job ended {other:?}"),
        }
    }
}
