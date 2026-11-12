//! Vorbis comment names for [`TagField`] and field edits on a comment list.

use parabolica_domain::tags::{FieldChange, FieldEdit, TagField};

use crate::TagError;
use crate::flac::VorbisComments;

/// The comment keys that hold `field`, canonical key first. A replacement removes every alias so a
/// file never keeps two disagreeing totals (for example `TRACKTOTAL` and `TOTALTRACKS`).
pub fn vorbis_keys(field: &TagField) -> Result<Vec<String>, TagError> {
    let keys: &[&str] = match field {
        TagField::Title => &["TITLE"],
        TagField::Artist => &["ARTIST"],
        TagField::Album => &["ALBUM"],
        TagField::AlbumArtist => &["ALBUMARTIST", "ALBUM ARTIST"],
        TagField::TrackNumber => &["TRACKNUMBER"],
        TagField::TrackTotal => &["TRACKTOTAL", "TOTALTRACKS"],
        TagField::DiscNumber => &["DISCNUMBER"],
        TagField::DiscTotal => &["DISCTOTAL", "TOTALDISCS"],
        TagField::Date => &["DATE"],
        TagField::Genre => &["GENRE"],
        TagField::Style => &["STYLE"],
        TagField::Label => &["LABEL"],
        TagField::CatalogNumber => &["CATALOGNUMBER"],
        TagField::Country => &["RELEASECOUNTRY"],
        TagField::ReplayGainTrackGain => &["REPLAYGAIN_TRACK_GAIN"],
        TagField::ReplayGainTrackPeak => &["REPLAYGAIN_TRACK_PEAK"],
        TagField::ReplayGainAlbumGain => &["REPLAYGAIN_ALBUM_GAIN"],
        TagField::ReplayGainAlbumPeak => &["REPLAYGAIN_ALBUM_PEAK"],
        TagField::Custom(name) => {
            let valid = !name.is_empty()
                && name
                    .bytes()
                    .all(|b| (0x20..=0x7d).contains(&b) && b != b'=');
            if !valid {
                return Err(TagError::InvalidField(name.clone()));
            }
            return Ok(vec![name.to_ascii_uppercase()]);
        }
    };
    Ok(keys.iter().map(|key| (*key).to_owned()).collect())
}

/// The field's values in file order, across all of its keys.
pub(crate) fn values(comments: &VorbisComments, field: &TagField) -> Result<Vec<String>, TagError> {
    let keys = vorbis_keys(field)?;
    Ok(comments
        .entries
        .iter()
        .filter(|entry| VorbisComments::has_key(entry, &keys))
        .filter_map(|entry| VorbisComments::value(entry))
        .collect())
}

/// Apply edits in order. Untouched entries keep their bytes and positions; a replacement takes
/// the position of the field's first existing entry. Replacing a field with the values it already
/// has under its canonical key changes nothing.
pub(crate) fn apply(
    comments: &mut VorbisComments,
    changes: &[FieldChange],
) -> Result<(), TagError> {
    for change in changes {
        let keys = vorbis_keys(&change.field)?;
        let values = match &change.edit {
            FieldEdit::Replace(values) => Some(values),
            FieldEdit::FillIfEmpty(values) => {
                let empty = comments
                    .entries
                    .iter()
                    .filter(|entry| VorbisComments::has_key(entry, &keys))
                    .all(|entry| VorbisComments::value(entry).is_none_or(|v| v.trim().is_empty()));
                empty.then_some(values)
            }
            FieldEdit::Remove => Some(&Vec::new()),
        };
        let Some(values) = values else {
            continue;
        };
        if values.iter().any(|value| value.contains('\0')) {
            return Err(TagError::InvalidValue(keys[0].clone()));
        }
        let replacement: Vec<Vec<u8>> = values
            .iter()
            .map(|value| format!("{}={value}", keys[0]).into_bytes())
            .collect();
        let existing: Vec<&Vec<u8>> = comments
            .entries
            .iter()
            .filter(|entry| VorbisComments::has_key(entry, &keys))
            .collect();
        if existing.len() == replacement.len()
            && existing
                .iter()
                .zip(&replacement)
                .all(|(old, new)| old.as_slice() == new.as_slice())
        {
            continue;
        }
        let at = comments
            .entries
            .iter()
            .position(|entry| VorbisComments::has_key(entry, &keys))
            .unwrap_or(comments.entries.len());
        comments
            .entries
            .retain(|entry| !VorbisComments::has_key(entry, &keys));
        let at = at.min(comments.entries.len());
        comments.entries.splice(at..at, replacement);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comments(entries: &[&str]) -> VorbisComments {
        VorbisComments {
            vendor: b"test".to_vec(),
            entries: entries.iter().map(|e| e.as_bytes().to_vec()).collect(),
        }
    }

    fn change(field: TagField, edit: FieldEdit) -> FieldChange {
        FieldChange { field, edit }
    }

    #[test]
    fn replace_keeps_untouched_entries_and_position_and_removes_aliases() {
        let mut c = comments(&[
            "TITLE=Old",
            "TOTALTRACKS=9",
            "custom_key=keep me",
            "REPLAYGAIN_TRACK_GAIN=-3.00 dB",
            "TRACKTOTAL=10",
        ]);
        apply(
            &mut c,
            &[
                change(TagField::TrackTotal, FieldEdit::Replace(vec!["12".into()])),
                change(TagField::Title, FieldEdit::Replace(vec!["New".into()])),
            ],
        )
        .unwrap();
        assert_eq!(
            c.entries,
            comments(&[
                "TITLE=New",
                "TRACKTOTAL=12",
                "custom_key=keep me",
                "REPLAYGAIN_TRACK_GAIN=-3.00 dB",
            ])
            .entries
        );
    }

    #[test]
    fn fill_if_empty_and_remove_and_multi_value() {
        let mut c = comments(&["GENRE=", "artist=A"]);
        apply(
            &mut c,
            &[
                change(
                    TagField::Genre,
                    FieldEdit::FillIfEmpty(vec!["Jazz".into(), "Soul".into()]),
                ),
                change(TagField::Artist, FieldEdit::FillIfEmpty(vec!["B".into()])),
                change(TagField::Date, FieldEdit::Remove),
            ],
        )
        .unwrap();
        assert_eq!(
            c.entries,
            comments(&["GENRE=Jazz", "GENRE=Soul", "artist=A"]).entries
        );
        assert_eq!(
            values(&c, &TagField::Genre).unwrap(),
            vec!["Jazz".to_owned(), "Soul".to_owned()]
        );
        apply(&mut c, &[change(TagField::Genre, FieldEdit::Remove)]).unwrap();
        assert_eq!(c.entries, comments(&["artist=A"]).entries);
    }

    #[test]
    fn identical_replacement_keeps_raw_bytes_and_bad_names_are_rejected() {
        let mut c = VorbisComments {
            vendor: Vec::new(),
            entries: vec![b"TITLE=Same".to_vec(), b"X=\xff\xfe".to_vec()],
        };
        let before = c.clone();
        apply(
            &mut c,
            &[change(
                TagField::Title,
                FieldEdit::Replace(vec!["Same".into()]),
            )],
        )
        .unwrap();
        assert_eq!(c, before);
        assert!(vorbis_keys(&TagField::Custom("A=B".into())).is_err());
        assert!(
            apply(
                &mut c,
                &[change(
                    TagField::Title,
                    FieldEdit::Replace(vec!["a\0b".into()])
                )]
            )
            .is_err()
        );
    }
}
