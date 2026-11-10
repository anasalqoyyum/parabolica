//! Turns the page's choices and the files read at load into reviewed per-file patches.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use parabolica_domain::files::FileSignature;
use parabolica_domain::ids::{ArtworkId, MediaFileId, ReleaseId};
use parabolica_domain::tags::{
    AlbumWrite, ArtworkChange, ArtworkDestination, FieldChange, FieldEdit, PictureType, TagField,
    TagPatch,
};
use parabolica_tags::FileTags;
use parabolica_tags::artwork::StagedImage;
use parabolica_tags::job::{SidecarPlan, TargetPlan, plan_sidecars};

/// Album-wide fields offered for editing, in display order.
pub const ALBUM_FIELDS: [TagField; 9] = [
    TagField::Album,
    TagField::AlbumArtist,
    TagField::Date,
    TagField::Genre,
    TagField::Style,
    TagField::Label,
    TagField::CatalogNumber,
    TagField::Country,
    TagField::DiscTotal,
];

/// Per-track fields that can be overridden.
pub const TRACK_FIELDS: [TagField; 3] = [TagField::Title, TagField::Artist, TagField::TrackNumber];

pub const PICTURE_TYPES: [PictureType; 3] = [
    PictureType::FrontCover,
    PictureType::BackCover,
    PictureType::Booklet,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Keep,
    Replace,
    FillEmpty,
}

#[derive(Debug, Clone)]
pub struct ReviewFile {
    pub file: MediaFileId,
    pub path: Option<PathBuf>,
    pub disc: u32,
    pub track: u32,
    pub title: String,
    pub state: FileState,
}

#[derive(Debug, Clone)]
pub enum FileState {
    Ready {
        tags: Box<FileTags>,
        reviewed: FileSignature,
    },
    /// Why the file cannot be written, shown in its row.
    Unavailable(String),
}

/// Existing sidecar images in one folder, by picture type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub path: PathBuf,
    pub existing: Vec<(PictureType, Vec<String>)>,
}

impl Folder {
    pub fn existing(&self, kind: PictureType) -> &[String] {
        self.existing
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(&[], |(_, names)| names.as_slice())
    }
}

#[derive(Debug, Clone)]
pub struct AlbumReview {
    pub album: ReleaseId,
    pub files: Vec<ReviewFile>,
    pub folders: Vec<Folder>,
}

impl AlbumReview {
    fn folder(&self, path: &Path) -> Option<&Folder> {
        self.folders.iter().find(|f| f.path == path)
    }

    /// Distinct values of `field` across readable files, in first-seen order.
    pub fn current_values(&self, field: &TagField) -> Vec<Vec<String>> {
        let mut distinct: Vec<Vec<String>> = Vec::new();
        for file in &self.files {
            if let FileState::Ready { tags, .. } = &file.state {
                let values = tags.values(field);
                if !distinct.contains(&values) {
                    distinct.push(values);
                }
            }
        }
        distinct
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChoice {
    pub field: TagField,
    pub mode: Mode,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackChoice {
    pub file: MediaFileId,
    pub included: bool,
    /// Non-empty overrides replace the field in this file only.
    pub overrides: Vec<(TagField, Vec<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtChoice {
    pub kind: PictureType,
    pub images: Vec<ArtworkId>,
    pub destination: ArtworkDestination,
    /// Replace pictures of this type already embedded; otherwise files that have one keep it.
    pub replace_embedded: bool,
    /// Replace existing sidecars such as `folder.jpg`; otherwise folders that have one keep it.
    pub replace_sidecars: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choices {
    pub fields: Vec<FieldChoice>,
    pub tracks: Vec<TrackChoice>,
    pub art: Vec<ArtChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewLine {
    pub subject: String,
    pub change: String,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub album: ReleaseId,
    pub targets: Vec<TargetPlan>,
    pub sidecars: Vec<SidecarPlan>,
    pub lines: Vec<ReviewLine>,
}

/// Split an entered value into a field's values. Multi-value entries are separated by `;`.
pub fn parse_values(text: &str) -> Vec<String> {
    text.split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn field_label(field: &TagField) -> &'static str {
    match field {
        TagField::Title => "Title",
        TagField::Artist => "Artist",
        TagField::Album => "Album",
        TagField::AlbumArtist => "Album artist",
        TagField::TrackNumber => "Track number",
        TagField::TrackTotal => "Track total",
        TagField::DiscNumber => "Disc number",
        TagField::DiscTotal => "Disc total",
        TagField::Date => "Date",
        TagField::Genre => "Genre",
        TagField::Style => "Style",
        TagField::Label => "Label",
        TagField::CatalogNumber => "Catalog number",
        TagField::Country => "Country",
        TagField::ReplayGainTrackGain => "Track gain",
        TagField::ReplayGainTrackPeak => "Track peak",
        TagField::ReplayGainAlbumGain => "Album gain",
        TagField::ReplayGainAlbumPeak => "Album peak",
        TagField::Custom(_) => "Custom",
    }
}

pub fn picture_label(kind: PictureType) -> &'static str {
    match kind {
        PictureType::FrontCover => "Front cover",
        PictureType::BackCover => "Back cover",
        PictureType::Booklet => "Booklet",
        PictureType::Other => "Other picture",
    }
}

pub fn show_values(values: &[String]) -> String {
    if values.is_empty() {
        "(empty)".into()
    } else {
        values.join("; ")
    }
}

pub fn plan(
    review: &AlbumReview,
    choices: &Choices,
    images: &HashMap<ArtworkId, StagedImage>,
) -> Result<Plan, String> {
    let mut targets = Vec::new();
    let mut excluded = BTreeSet::new();
    let mut lines = Vec::new();
    for file in &review.files {
        let FileState::Ready { tags, reviewed } = &file.state else {
            continue;
        };
        let Some(path) = &file.path else {
            continue;
        };
        let track = choices.tracks.iter().find(|t| t.file == file.file);
        if track.is_some_and(|t| !t.included) {
            excluded.insert(file.file);
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut fields = Vec::new();
        let mut edits: Vec<(TagField, Mode, Vec<String>)> = choices
            .fields
            .iter()
            .map(|c| (c.field.clone(), c.mode, c.values.clone()))
            .collect();
        for (field, values) in track.map_or(&[][..], |t| &t.overrides) {
            if !values.is_empty() {
                edits.retain(|(f, _, _)| f != field);
                edits.push((field.clone(), Mode::Replace, values.clone()));
            }
        }
        for (field, mode, values) in edits {
            let current = tags.values(&field);
            let edit = match mode {
                Mode::Keep => continue,
                Mode::Replace if values.is_empty() && current.is_empty() => continue,
                Mode::Replace if values.is_empty() => FieldEdit::Remove,
                Mode::Replace if values == current => continue,
                Mode::Replace => FieldEdit::Replace(values.clone()),
                Mode::FillEmpty
                    if values.is_empty() || current.iter().any(|v| !v.trim().is_empty()) =>
                {
                    continue;
                }
                Mode::FillEmpty => FieldEdit::FillIfEmpty(values.clone()),
            };
            lines.push(ReviewLine {
                subject: name.clone(),
                change: format!(
                    "{}: {} → {}",
                    field_label(&field),
                    show_values(&current),
                    show_values(&values)
                ),
            });
            fields.push(FieldChange { field, edit });
        }
        let folder = path.parent().and_then(|dir| review.folder(dir));
        let mut artwork = Vec::new();
        for choice in &choices.art {
            if choice.images.is_empty() {
                continue;
            }
            let existing_embedded = tags.has_picture(choice.kind);
            let embed = choice.destination != ArtworkDestination::BesideFiles
                && (choice.replace_embedded || !existing_embedded);
            let beside = choice.destination != ArtworkDestination::Embed
                && (choice.replace_sidecars
                    || folder.is_none_or(|f| f.existing(choice.kind).is_empty()));
            let destination = match (embed, beside) {
                (true, true) => ArtworkDestination::Both,
                (true, false) => ArtworkDestination::Embed,
                (false, true) => ArtworkDestination::BesideFiles,
                (false, false) => continue,
            };
            for id in &choice.images {
                if !images.contains_key(id) {
                    return Err("A selected image is no longer staged; choose it again".into());
                }
                artwork.push(ArtworkChange {
                    artwork: *id,
                    picture_type: choice.kind,
                    destination,
                });
            }
            if embed {
                lines.push(ReviewLine {
                    subject: name.clone(),
                    change: format!(
                        "{}: {} {}",
                        picture_label(choice.kind),
                        if existing_embedded {
                            "replace embedded"
                        } else {
                            "embed"
                        },
                        describe_images(&choice.images, images)
                    ),
                });
            }
        }
        if fields.is_empty() && artwork.is_empty() {
            continue;
        }
        targets.push(TargetPlan {
            patch: TagPatch {
                target: file.file,
                reviewed: reviewed.clone(),
                fields,
                artwork,
            },
            path: path.clone(),
        });
    }
    if targets.is_empty() {
        return Err("No changes to write".into());
    }
    AlbumWrite::new(
        review.album,
        targets.iter().map(|t| t.patch.clone()).collect(),
        excluded,
    )
    .map_err(|error| error.to_string())?;
    let sidecars = plan_sidecars(&targets, images, |dir, kind| {
        review
            .folder(dir)
            .map(|f| f.existing(kind).to_vec())
            .unwrap_or_default()
    });
    for sidecar in &sidecars {
        lines.push(ReviewLine {
            subject: sidecar.directory.display().to_string(),
            change: if sidecar.replace.is_empty() {
                format!("Save {}", sidecar.file_name)
            } else {
                format!(
                    "Save {}, replacing {}",
                    sidecar.file_name,
                    sidecar.replace.join(", ")
                )
            },
        });
    }
    Ok(Plan {
        album: review.album,
        targets,
        sidecars,
        lines,
    })
}

fn describe_images(ids: &[ArtworkId], images: &HashMap<ArtworkId, StagedImage>) -> String {
    ids.iter()
        .filter_map(|id| images.get(id))
        .map(|image| {
            format!(
                "{} × {} {}",
                image.width(),
                image.height(),
                image.kind().extension().to_uppercase()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}
