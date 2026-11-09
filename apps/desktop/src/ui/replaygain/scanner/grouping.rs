use parabolica_domain::ids::{MediaFileId, ReleaseId};
use parabolica_storage::{
    Storage, StorageError,
    replaygain::{ReviewedGroup, ScanCandidate, ScanMember, ScanMode},
};
use std::collections::{BTreeMap, HashSet};

pub fn review(
    storage: &Storage,
    all: &[ScanCandidate],
    selected: &HashSet<MediaFileId>,
    mode: ScanMode,
) -> Result<Vec<ReviewedGroup>, StorageError> {
    let mut files = HashSet::new();
    let mut by_album = BTreeMap::<ReleaseId, Vec<&ScanCandidate>>::new();
    let mut disc_counts = BTreeMap::<(ReleaseId, u32), usize>::new();
    for candidate in all {
        files.insert(candidate.file);
        if mode != ScanMode::Tracks {
            by_album.entry(candidate.album).or_default().push(candidate);
            *disc_counts
                .entry((candidate.album, candidate.disc))
                .or_default() += 1;
        }
    }
    if !selected.is_subset(&files) {
        return Err(StorageError::Conflict("Selected tracks were removed from the library. Make a new selection and review its groups.".into()));
    }
    let mut groups = BTreeMap::<String, Vec<&ScanCandidate>>::new();
    for item in all.iter().filter(|c| selected.contains(&c.file)) {
        let key = match mode {
            ScanMode::Tracks => item.file.to_string(),
            ScanMode::Album => "one".into(),
            ScanMode::Tags => item.album.to_string(),
            ScanMode::Disc => format!("{}:{}", item.album, item.disc),
        };
        groups.entry(key).or_default().push(item);
    }
    let mut result = Vec::new();
    for members in groups.values() {
        let mut album_counts = BTreeMap::<ReleaseId, usize>::new();
        if mode != ScanMode::Tracks {
            for member in members {
                *album_counts.entry(member.album).or_default() += 1;
            }
        }
        let mut warnings = Vec::new();
        if members.iter().any(|m| !m.available) {
            warnings.push("Missing or unreadable tracks are included".to_string());
        }
        for (album, included) in &album_counts {
            let full = &by_album[album];
            if *included < full.len() && mode != ScanMode::Disc {
                warnings.push(format!(
                    "Selected {included} of {} library tracks in {}",
                    full.len(),
                    full[0].album_title
                ));
            }
        }
        if mode != ScanMode::Tracks {
            let mut discs = BTreeMap::<(ReleaseId, u32), Vec<&ScanCandidate>>::new();
            let mut disc_total = 0;
            for member in members {
                discs
                    .entry((member.album, member.disc))
                    .or_default()
                    .push(member);
                let tags = storage.media_file_tags(member.file)?;
                let number = |fields: &[&str]| {
                    tags.iter()
                        .find(|t| fields.contains(&t.field.as_str()))
                        .and_then(|t| t.value.parse::<u32>().ok())
                        .unwrap_or(0)
                };
                disc_total = disc_total.max(number(&["DISCTOTAL", "TOTALDISCS"]));
            }
            for ((album, disc), tracks) in &discs {
                if mode == ScanMode::Disc {
                    let known = disc_counts[&(*album, *disc)];
                    if tracks.len() < known {
                        warnings.push(format!(
                            "Disc {disc}: selected {} of {known} library tracks",
                            tracks.len()
                        ));
                    }
                }
                let mut total = 0;
                for member in tracks {
                    let tags = storage.media_file_tags(member.file)?;
                    for tag in tags {
                        if ["TRACKTOTAL", "TOTALTRACKS"].contains(&tag.field.as_str()) {
                            total = total.max(tag.value.parse::<u32>().unwrap_or(0));
                        }
                        if tag.field == "TRACKNUMBER" {
                            total = total.max(
                                tag.value
                                    .split('/')
                                    .nth(1)
                                    .and_then(|n| n.parse::<u32>().ok())
                                    .unwrap_or(0),
                            );
                        }
                    }
                }
                let numbers = tracks
                    .iter()
                    .map(|m| m.track)
                    .filter(|n| *n > 0)
                    .collect::<HashSet<_>>();
                let max = numbers.iter().max().copied().unwrap_or(0).max(total);
                if max > 0 && (1..=max).any(|n| !numbers.contains(&n)) {
                    warnings.push(format!("Disc {disc}: suspected missing tracks, {} selected, declared/highest track {max}",tracks.len()));
                }
                if numbers.len() != tracks.len() {
                    warnings.push(format!("Disc {disc}: missing or duplicate track numbers"));
                }
            }
            if mode != ScanMode::Disc && disc_total > 0 && discs.len() < disc_total as usize {
                warnings.push(format!(
                    "Suspected missing discs: {} of {disc_total} selected",
                    discs.len()
                ));
            }
        }
        warnings.sort();
        warnings.dedup();
        let first = members[0];
        let name = match mode {
            ScanMode::Tracks => first.title.clone(),
            ScanMode::Album => "Selected album".into(),
            ScanMode::Tags => format!(
                "{} · {}{}",
                first.album_artist,
                first.album_title,
                if first.edition.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", first.edition)
                }
            ),
            ScanMode::Disc => format!(
                "{} · {} · Disc {}",
                first.album_artist, first.album_title, first.disc
            ),
        };
        result.push(ReviewedGroup {
            name,
            warning: warnings.join("; "),
            members: members
                .iter()
                .map(|m| ScanMember {
                    file: m.file,
                    title: format!("{}.{} {}", m.disc, m.track, m.title),
                })
                .collect(),
            albums: album_counts
                .into_keys()
                .map(|album| (album, by_album[&album].iter().map(|c| c.file).collect()))
                .collect(),
        });
    }
    Ok(result)
}
