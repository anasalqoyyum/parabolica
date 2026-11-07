use super::*;
use crate::ids::MediaFileId;

fn sources(count: usize) -> Vec<PlayableSource> {
    (0..count)
        .map(|_| PlayableSource::LocalFile(MediaFileId::generate()))
        .collect()
}

#[test]
fn shuffle_next_move_removal_and_restore_keep_stable_ids() {
    let mut queue = QueueSession::default();
    let ids = queue.insert(sources(8), InsertPosition::End);
    queue.record_started(ids[3]).unwrap();
    queue
        .apply(QueueCommand::SetShuffle {
            seed: Some(u64::MAX),
        })
        .unwrap();
    assert_eq!(queue.effective_order()[0], ids[3]);
    let restored = QueueSession::restore(
        queue.entries().to_vec(),
        queue.current(),
        queue.repeat(),
        queue.shuffle().cloned(),
        queue.history().to_vec(),
    )
    .unwrap();
    assert_eq!(restored, queue);
    let mut same = QueueSession::restore(
        queue.entries().to_vec(),
        queue.current(),
        queue.repeat(),
        None,
        queue.history().to_vec(),
    )
    .unwrap();
    same.apply(QueueCommand::SetShuffle {
        seed: Some(u64::MAX),
    })
    .unwrap();
    assert_eq!(same.effective_order(), queue.effective_order());
    let duplicate = queue.entries()[0].source.clone();
    let inserted = queue.insert([duplicate.clone(), duplicate], InsertPosition::AfterCurrent);
    assert_ne!(inserted[0], inserted[1]);
    assert_eq!(&queue.effective_order()[1..3], inserted.as_slice());
    queue
        .apply(QueueCommand::Move {
            entry: inserted[1],
            to_index: 1,
        })
        .unwrap();
    assert_eq!(queue.effective_order()[1], inserted[1]);
    queue.apply(QueueCommand::Remove(ids[3])).unwrap();
    assert_eq!(queue.current(), Some(inserted[1]));
}

#[test]
fn explicit_navigation_ignores_repeat_one_and_previous_uses_actual_starts() {
    let mut queue = QueueSession::default();
    let ids = queue.insert(sources(4), InsertPosition::End);
    queue.record_started(ids[0]).unwrap();
    queue.record_started(ids[3]).unwrap();
    queue
        .apply(QueueCommand::SetRepeat(RepeatMode::One))
        .unwrap();
    assert_eq!(queue.next(true), Some(ids[3]));
    assert_eq!(queue.previous(), Some(ids[0]));
    queue.record_started(ids[0]).unwrap();
    assert_eq!(queue.next(false), Some(ids[1]));
    queue
        .apply(QueueCommand::SetRepeat(RepeatMode::All))
        .unwrap();
    queue.apply(QueueCommand::Jump(ids[3])).unwrap();
    assert_eq!(queue.next(false), Some(ids[0]));
    queue.apply(QueueCommand::Jump(ids[3])).unwrap();
    queue.apply(QueueCommand::Remove(ids[3])).unwrap();
    assert_eq!(queue.current(), None);
}

#[test]
fn command_sequences_preserve_queue_and_shuffle_permutations() {
    for seed in 0..32 {
        let mut state = seed;
        let mut queue = QueueSession::default();
        for _ in 0..400 {
            let random = splitmix64(&mut state);
            let order = queue.effective_order();
            let selected = order
                .get((random as usize).wrapping_div(7) % order.len().max(1))
                .copied();
            let command = match (random % 9, selected) {
                (0, _) => QueueCommand::SetShuffle { seed: Some(random) },
                (1, _) => QueueCommand::SetShuffle { seed: None },
                (2, Some(entry)) => QueueCommand::Move {
                    entry,
                    to_index: random as usize % 100,
                },
                (3, Some(entry)) => QueueCommand::Remove(entry),
                (4, Some(entry)) => QueueCommand::Jump(entry),
                (5, _) => QueueCommand::SetRepeat(RepeatMode::All),
                (6, _) => QueueCommand::PlayNow(sources(2)),
                _ => QueueCommand::Insert {
                    sources: sources(1),
                    at: InsertPosition::AfterCurrent,
                },
            };
            queue.apply(command).unwrap();
            if random.is_multiple_of(3)
                && let Some(id) = queue.next(false)
            {
                queue.record_started(id).unwrap();
            }
            if random.is_multiple_of(5) {
                queue.previous();
            }
            let ids: HashSet<_> = queue.entries().iter().map(|entry| entry.id).collect();
            assert_eq!(ids.len(), queue.entries().len());
            assert!(queue.current().is_none_or(|id| ids.contains(&id)));
            assert_eq!(
                queue.effective_order().into_iter().collect::<HashSet<_>>(),
                ids
            );
            assert_eq!(
                QueueSession::restore(
                    queue.entries().to_vec(),
                    queue.current(),
                    queue.repeat(),
                    queue.shuffle().cloned(),
                    queue.history().to_vec()
                )
                .unwrap(),
                queue
            );
        }
    }
}

#[test]
fn previous_navigation_keeps_a_bounded_recent_window() {
    let mut queue = QueueSession::default();
    let ids = queue.insert(sources(2), InsertPosition::End);
    for index in 0..2001 {
        queue.record_started(ids[index % 2]).unwrap();
    }
    assert_eq!(queue.history().len(), 1000);
    assert_eq!(queue.history().last(), Some(&ids[0]));
}
