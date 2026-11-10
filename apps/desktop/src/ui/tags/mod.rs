//! Files & tags: album fields, per-track overrides and artwork, reviewed and written file by file.

pub mod discogs;
mod review;
mod service;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, PathPromptOptions, Render, Role, SharedString, Subscription,
    Task, TestSupportExt as _, Window, div, px,
};
use parabolica_domain::ids::{MediaFileId, ReleaseId};
use parabolica_domain::jobs::JobState;
use parabolica_domain::playback::TransportState;
use parabolica_domain::tags::{ArtworkDestination, PictureType, TagField};
use parabolica_storage::tag_writes::{TagSidecarState, TagTargetState, TagWriteRecord};

use crate::ui::library::LibraryView;
use crate::ui::player::Player;
use review::{
    ALBUM_FIELDS, AlbumReview, ArtChoice, Choices, FieldChoice, FileState, Mode, PICTURE_TYPES,
    Plan, TRACK_FIELDS, TrackChoice, field_label, parse_values, picture_label, show_values,
};
use service::{Command, Status, TagService};

const MODES: [(Mode, &str); 3] = [
    (Mode::Keep, "Keep"),
    (Mode::Replace, "Replace"),
    (Mode::FillEmpty, "Fill empty"),
];
const DESTINATIONS: [(ArtworkDestination, &str); 3] = [
    (ArtworkDestination::Embed, "Embed in files"),
    (ArtworkDestination::BesideFiles, "Save beside files"),
    (ArtworkDestination::Both, "Both"),
];

#[derive(Debug, Clone, Copy)]
struct ArtSetting {
    destination: ArtworkDestination,
    replace_embedded: bool,
    replace_sidecars: bool,
}

pub struct FilesAndTags {
    service: Option<Arc<TagService>>,
    player: Entity<Player>,
    library: Entity<LibraryView>,
    status: Status,
    revision: u64,
    writes_finished: u64,
    album: Option<ReleaseId>,
    /// The review the current choices were made against.
    shown: Option<Arc<AlbumReview>>,
    modes: Vec<Mode>,
    inputs: HashMap<String, Entity<InputState>>,
    excluded: HashSet<MediaFileId>,
    art: Vec<ArtSetting>,
    plan: Option<Result<Plan, String>>,
    error: Option<String>,
    subscriptions: Vec<Subscription>,
    _poll: Task<()>,
}

impl FilesAndTags {
    pub fn new(
        player: Entity<Player>,
        library: Entity<LibraryView>,
        cx: &mut Context<Self>,
    ) -> Self {
        let service = crate::ui::library::storage(cx)
            .map(TagService::start)
            .map(Arc::new);
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            service,
            player,
            library,
            status: Status::default(),
            revision: 0,
            writes_finished: 0,
            album: None,
            shown: None,
            modes: vec![Mode::Keep; ALBUM_FIELDS.len()],
            inputs: HashMap::new(),
            excluded: HashSet::new(),
            art: default_art(),
            plan: None,
            error: None,
            subscriptions: Vec::new(),
            _poll: poll,
        }
    }

    pub(crate) fn open_album(&mut self, album: ReleaseId, cx: &mut Context<Self>) {
        if self.album != Some(album) {
            self.album = Some(album);
            self.reset_choices();
        }
        self.send(Command::Load(album), cx);
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        let Some(service) = self.service.clone() else {
            return;
        };
        let open = self
            .player
            .read(cx)
            .output_snapshot()
            .filter(|s| s.playback.transport != TransportState::Stopped)
            .and_then(|s| s.path.clone());
        service.set_in_use(open);
        let status = service.status();
        if status.revision == self.revision {
            return;
        }
        self.revision = status.revision;
        if status.writes_finished != self.writes_finished {
            self.writes_finished = status.writes_finished;
            if let Some(library) = self.library.read(cx).service.clone()
                && let Err(error) = library.send(parabolica_library::service::Command::Rescan)
            {
                self.error = Some(error);
            }
        }
        let review_changed = match (&status.review, &self.shown) {
            (Some(new), Some(old)) => !Arc::ptr_eq(new, old),
            (new, old) => new.is_some() != old.is_some(),
        };
        if review_changed {
            // A write can regroup the files into a new album; the review follows them.
            if let Some(review) = &status.review {
                self.album = Some(review.album);
            }
            if status
                .review
                .as_ref()
                .is_some_and(|r| self.shown.as_ref().is_none_or(|old| old.album != r.album))
            {
                self.reset_choices();
            }
            self.shown = status.review.clone();
            self.plan = None;
        }
        self.status = status;
        cx.notify();
    }

    fn reset_choices(&mut self) {
        self.modes = vec![Mode::Keep; ALBUM_FIELDS.len()];
        self.inputs.clear();
        self.subscriptions.clear();
        self.excluded.clear();
        self.art = default_art();
        self.plan = None;
        self.shown = None;
    }

    fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        self.error = None;
        if let Some(service) = &self.service {
            match service.send(command) {
                Ok(()) => self.status = service.status(),
                Err(error) => self.error = Some(error),
            }
        } else {
            self.error = Some("Library database unavailable".into());
        }
        cx.notify();
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.plan = None;
        cx.notify();
    }

    fn input(
        &mut self,
        key: String,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.inputs.get(&key) {
            return input.clone();
        }
        let placeholder = SharedString::from(placeholder.to_owned());
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let field = ALBUM_FIELDS
            .iter()
            .position(|f| key == format!("album-{}", field_key(f)));
        self.subscriptions.push(
            cx.subscribe(&input, move |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Some(index) = field
                        && this.modes[index] == Mode::Keep
                    {
                        this.modes[index] = Mode::Replace;
                    }
                    this.changed(cx);
                }
            }),
        );
        self.inputs.insert(key, input.clone());
        input
    }

    fn value(&self, key: &str, cx: &App) -> String {
        self.inputs
            .get(key)
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default()
    }

    fn choices(&self, review: &AlbumReview, cx: &App) -> Choices {
        Choices {
            fields: ALBUM_FIELDS
                .iter()
                .zip(&self.modes)
                .map(|(field, mode)| FieldChoice {
                    field: field.clone(),
                    mode: *mode,
                    values: parse_values(&self.value(&format!("album-{}", field_key(field)), cx)),
                })
                .collect(),
            tracks: review
                .files
                .iter()
                .map(|file| TrackChoice {
                    file: file.file,
                    included: !self.excluded.contains(&file.file),
                    overrides: TRACK_FIELDS
                        .iter()
                        .map(|field| {
                            (
                                field.clone(),
                                parse_values(&self.value(&track_key(file.file, field), cx)),
                            )
                        })
                        .collect(),
                })
                .collect(),
            art: PICTURE_TYPES
                .iter()
                .zip(&self.art)
                .map(|(kind, setting)| ArtChoice {
                    kind: *kind,
                    images: self
                        .status
                        .staged
                        .iter()
                        .filter(|s| s.kind == *kind)
                        .map(|s| s.id)
                        .collect(),
                    destination: setting.destination,
                    replace_embedded: setting.replace_embedded,
                    replace_sidecars: setting.replace_sidecars,
                })
                .collect(),
        }
    }

    fn review(&mut self, cx: &mut Context<Self>) {
        let Some(review) = self.status.review.clone() else {
            return;
        };
        let images = self
            .status
            .staged
            .iter()
            .map(|s| (s.id, s.image.clone()))
            .collect();
        self.plan = Some(review::plan(&review, &self.choices(&review, cx), &images));
        cx.notify();
    }

    fn choose_image(&mut self, kind: PictureType, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(format!("Choose {}", picture_label(kind).to_lowercase()).into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = prompt.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    this.plan = None;
                    this.send(Command::Stage(path, kind), cx)
                });
            }
        })
        .detach();
    }

    fn render_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.status.busy;
        v_flex()
            .gap_2()
            .when(busy, |column| {
                let text = match self.status.progress {
                    Some((done, total)) => format!("{} {done} of {total}", self.status.operation),
                    None => self.status.operation.clone(),
                };
                column.child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(label("tags-operation", text))
                        .when(self.status.cancellable, |row| {
                            row.child(Button::new("tags-cancel").label("Cancel").on_click(
                                cx.listener(|this, _, _, _| {
                                    if let Some(service) = &this.service {
                                        service.cancel();
                                    }
                                }),
                            ))
                        }),
                )
            })
            .when_some(
                self.error.clone().or_else(|| self.status.error.clone()),
                |column, error| {
                    column.child(
                        div()
                            .id("tags-error")
                            .test_support()
                            .role(Role::Alert)
                            .aria_label(error.clone())
                            .text_color(cx.theme().warning)
                            .child(error),
                    )
                },
            )
            .into_any_element()
    }

    fn render_fields(
        &mut self,
        review: &AlbumReview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let busy = self.status.busy;
        let mut rows = Vec::new();
        for (index, field) in ALBUM_FIELDS.iter().enumerate() {
            let key = field_key(field);
            let input = self.input(format!("album-{key}"), multi_hint(field), window, cx);
            let current = review.current_values(field);
            let current = match current.as_slice() {
                [] => "(no readable files)".to_owned(),
                [one] => show_values(one),
                many => format!("Mixed: {} values", many.len()),
            };
            rows.push(
                h_flex()
                    .id(SharedString::from(format!("tags-field-{key}")))
                    .gap_3()
                    .py_1()
                    .items_center()
                    .flex_wrap()
                    .child(
                        label(format!("tags-field-name-{key}"), field_label(field).into())
                            .w(px(130.))
                            .font_semibold(),
                    )
                    .child(
                        label(format!("tags-field-current-{key}"), current)
                            .w(px(220.))
                            .truncate()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        Input::new(&input)
                            .id(SharedString::from(format!("tags-field-input-{key}")))
                            .aria_label(format!("New {}", field_label(field).to_lowercase()))
                            .w(px(240.)),
                    )
                    .child(
                        RadioGroup::horizontal(SharedString::from(format!(
                            "tags-field-mode-{key}"
                        )))
                        .selected_index(MODES.iter().position(|(m, _)| *m == self.modes[index]))
                        .disabled(busy)
                        .children(MODES.iter().map(|(mode, text)| {
                            Radio::new(SharedString::from(format!("tags-mode-{key}-{mode:?}")))
                                .label(*text)
                        }))
                        .on_change(cx.listener(
                            move |this, choice: &usize, _, cx| {
                                this.modes[index] = MODES[*choice].0;
                                this.changed(cx);
                            },
                        )),
                    )
                    .into_any_element(),
            );
        }
        section("tags-album-fields", "Album fields", cx)
            .children(rows)
            .into_any_element()
    }

    fn render_tracks(
        &mut self,
        review: &AlbumReview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut rows = Vec::new();
        for file in &review.files {
            let id = file.file;
            let name = file
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.title.clone());
            let heading = format!("{}.{:02}  {name}", file.disc, file.track);
            let mut row = v_flex()
                .id(SharedString::from(format!("tags-track-{id}")))
                .gap_2()
                .py_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    label(format!("tags-track-name-{id}"), heading)
                        .font_semibold()
                        .truncate(),
                );
            match &file.state {
                FileState::Unavailable(reason) => {
                    row = row.child(
                        label(format!("tags-track-unavailable-{id}"), reason.clone())
                            .text_color(cx.theme().warning),
                    );
                }
                FileState::Ready { tags, .. } => {
                    let current = TRACK_FIELDS
                        .iter()
                        .map(|f| format!("{}: {}", field_label(f), show_values(&tags.values(f))))
                        .collect::<Vec<_>>()
                        .join(" · ");
                    let mut inputs = h_flex().gap_2().flex_wrap().child(
                        Checkbox::new(SharedString::from(format!("tags-include-{id}")))
                            .label("Include")
                            .checked(!self.excluded.contains(&id))
                            .disabled(self.status.busy)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                if *checked {
                                    this.excluded.remove(&id);
                                } else {
                                    this.excluded.insert(id);
                                }
                                this.changed(cx);
                            })),
                    );
                    for field in TRACK_FIELDS {
                        let input =
                            self.input(track_key(id, &field), field_label(&field), window, cx);
                        inputs = inputs.child(
                            Input::new(&input)
                                .id(SharedString::from(format!(
                                    "tags-track-input-{id}-{}",
                                    field_key(&field)
                                )))
                                .aria_label(format!(
                                    "New {} for {name}",
                                    field_label(&field).to_lowercase()
                                ))
                                .w(px(200.)),
                        );
                    }
                    row = row
                        .child(
                            label(format!("tags-track-current-{id}"), current)
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .truncate(),
                        )
                        .child(inputs);
                }
            }
            rows.push(row.into_any_element());
        }
        section("tags-tracks", "Tracks", cx)
            .child(
                label(
                    "tags-track-hint",
                    "A track value replaces the album choice for that file.".into(),
                )
                .text_sm()
                .text_color(cx.theme().muted_foreground),
            )
            .children(rows)
            .into_any_element()
    }

    fn render_artwork(&self, review: &AlbumReview, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.status.busy;
        let readable = review
            .files
            .iter()
            .filter(|f| matches!(f.state, FileState::Ready { .. }))
            .count();
        let mut column = section("tags-artwork", "Artwork", cx);
        for (index, kind) in PICTURE_TYPES.into_iter().enumerate() {
            let key = picture_key(kind);
            let setting = self.art[index];
            let embedded = review
                .files
                .iter()
                .filter(
                    |f| matches!(&f.state, FileState::Ready { tags, .. } if tags.has_picture(kind)),
                )
                .count();
            let sidecars: Vec<String> = review
                .folders
                .iter()
                .flat_map(|f| f.existing(kind).iter().cloned())
                .collect();
            let staged: Vec<_> = self
                .status
                .staged
                .iter()
                .filter(|s| s.kind == kind)
                .cloned()
                .collect();
            let current = format!(
                "Embedded in {embedded} of {readable} files · {}",
                if sidecars.is_empty() {
                    "No image beside files".to_owned()
                } else {
                    format!("Beside files: {}", sidecars.join(", "))
                }
            );
            let mut block = v_flex()
                .id(SharedString::from(format!("tags-art-{key}")))
                .gap_2()
                .py_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    label(format!("tags-art-title-{key}"), picture_label(kind).into())
                        .font_semibold(),
                )
                .child(
                    label(format!("tags-art-current-{key}"), current)
                        .text_sm()
                        .text_color(cx.theme().muted_foreground),
                );
            for image in &staged {
                let id = image.id;
                block = block.child(
                    h_flex()
                        .id(SharedString::from(format!("tags-art-staged-{id}")))
                        .gap_3()
                        .items_center()
                        .child(label(
                            format!("tags-art-staged-name-{id}"),
                            format!(
                                "{} · {} × {} {}",
                                image.name,
                                image.image.width(),
                                image.image.height(),
                                image.image.kind().extension().to_uppercase()
                            ),
                        ))
                        .child(
                            Button::new(SharedString::from(format!("tags-art-remove-{id}")))
                                .ghost()
                                .label("Remove")
                                .accessibility_label(format!("Remove {}", image.name))
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.plan = None;
                                    this.send(Command::Unstage(id), cx);
                                })),
                        ),
                );
            }
            block = block
                .child(
                    Button::new(SharedString::from(format!("tags-art-choose-{key}")))
                        .label(if kind == PictureType::Booklet {
                            "Add booklet page"
                        } else if staged.is_empty() {
                            "Choose image"
                        } else {
                            "Choose another image"
                        })
                        .accessibility_label(format!(
                            "Choose {} image",
                            picture_label(kind).to_lowercase()
                        ))
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| this.choose_image(kind, cx))),
                )
                .when(!staged.is_empty(), |block| {
                    block
                        .child(
                            RadioGroup::horizontal(SharedString::from(format!(
                                "tags-art-destination-{key}"
                            )))
                            .selected_index(
                                DESTINATIONS
                                    .iter()
                                    .position(|(d, _)| *d == setting.destination),
                            )
                            .disabled(busy)
                            .children(DESTINATIONS.iter().map(|(d, text)| {
                                Radio::new(SharedString::from(format!("tags-art-{key}-{d:?}")))
                                    .label(*text)
                            }))
                            .on_change(cx.listener(
                                move |this, choice: &usize, _, cx| {
                                    this.art[index].destination = DESTINATIONS[*choice].0;
                                    this.changed(cx);
                                },
                            )),
                        )
                        .when(
                            embedded > 0 && setting.destination != ArtworkDestination::BesideFiles,
                            |block| {
                                block.child(
                                    Checkbox::new(SharedString::from(format!(
                                        "tags-art-replace-embedded-{key}"
                                    )))
                                    .label(format!(
                                        "Replace embedded {} in {embedded} files",
                                        picture_label(kind).to_lowercase()
                                    ))
                                    .checked(setting.replace_embedded)
                                    .disabled(busy)
                                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                        this.art[index].replace_embedded = *checked;
                                        this.changed(cx);
                                    })),
                                )
                            },
                        )
                        .when(
                            !sidecars.is_empty()
                                && setting.destination != ArtworkDestination::Embed,
                            |block| {
                                block.child(
                                    Checkbox::new(SharedString::from(format!(
                                        "tags-art-replace-sidecars-{key}"
                                    )))
                                    .label(format!("Replace {}", sidecars.join(", ")))
                                    .checked(setting.replace_sidecars)
                                    .disabled(busy)
                                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                        this.art[index].replace_sidecars = *checked;
                                        this.changed(cx);
                                    })),
                                )
                            },
                        )
                });
            column = column.child(block);
        }
        column.into_any_element()
    }

    fn render_review(&self, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.status.busy;
        let mut column = section("tags-review", "Review", cx).child(
            Button::new("tags-review-button")
                .label("Review changes")
                .disabled(busy || self.status.review.is_none())
                .on_click(cx.listener(|this, _, _, cx| this.review(cx))),
        );
        match &self.plan {
            None => {}
            Some(Err(message)) => {
                column = column.child(label("tags-review-message", message.clone()));
            }
            Some(Ok(plan)) => {
                let files = plan.targets.len();
                column = column
                    .children(plan.lines.iter().enumerate().map(|(i, line)| {
                        label(
                            format!("tags-review-line-{i}"),
                            format!("{} — {}", line.subject, line.change),
                        )
                        .text_sm()
                        .into_any_element()
                    }))
                    .child(
                        label(
                            "tags-review-destination",
                            "Files are rewritten in place. Originals are kept as backups in the app data folder until you delete them.".into(),
                        )
                        .text_sm()
                        .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        Button::new("tags-apply")
                            .primary()
                            .label(if files == 1 {
                                "Write 1 file".to_owned()
                            } else {
                                format!("Write {files} files")
                            })
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(Ok(plan)) = this.plan.take() {
                                    this.send(Command::Apply(Box::new(plan)), cx);
                                }
                            })),
                    );
            }
        }
        column.into_any_element()
    }

    fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.status.busy;
        let mut column = section("tags-history", "Write history", cx);
        if self.status.history.is_empty() {
            return column
                .child(label(
                    "tags-history-empty",
                    "No writes for this album".into(),
                ))
                .into_any_element();
        }
        for record in self.status.history.iter() {
            column = column.child(render_record(record, busy, cx));
        }
        column.into_any_element()
    }
}

fn render_record(
    record: &TagWriteRecord,
    busy: bool,
    cx: &mut Context<FilesAndTags>,
) -> AnyElement {
    let id = record.id;
    let finished = !matches!(record.state, JobState::Queued | JobState::Running(_));
    let retryable = record
        .targets
        .iter()
        .any(|(_, s)| service::target_from_storage(s).retryable())
        || record
            .sidecars
            .iter()
            .any(|(_, s)| service::sidecar_from_storage(s).retryable());
    let restorable = !record.backups_discarded;
    let has_backups = restorable
        && (record
            .targets
            .iter()
            .any(|(_, s)| matches!(s, TagTargetState::Written { .. }))
            || record
                .sidecars
                .iter()
                .any(|(s, state)| *state == TagSidecarState::Written && !s.replace.is_empty()));
    let heading = format!(
        "{} · {}{}",
        write_time(record.created_at_ms),
        job_label(&record.state),
        if record.retry_of.is_some() {
            " · Retry"
        } else {
            ""
        }
    );
    v_flex()
        .id(SharedString::from(format!("tags-write-{id}")))
        .gap_1()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .flex_wrap()
                .child(label(format!("tags-write-heading-{id}"), heading).font_semibold())
                .when(finished && retryable, |row| {
                    row.child(
                        Button::new(SharedString::from(format!("tags-retry-{id}")))
                            .label("Retry unfinished")
                            .disabled(busy)
                            .on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.send(Command::Retry(id), cx)
                                }),
                            ),
                    )
                })
                .when(finished && has_backups, |row| {
                    row.child(
                        Button::new(SharedString::from(format!("tags-discard-{id}")))
                            .ghost()
                            .label("Delete backups")
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.send(Command::DiscardBackups(id), cx)
                            })),
                    )
                }),
        )
        .children(record.targets.iter().map(|(target, state)| {
            let file = target.patch.target;
            let name = target
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            h_flex()
                .id(SharedString::from(format!("tags-write-{id}-{file}")))
                .gap_3()
                .items_center()
                .child(
                    label(
                        format!("tags-outcome-{id}-{file}"),
                        format!("{name}: {}", target_label(state)),
                    )
                    .text_sm(),
                )
                .when(
                    finished && restorable && matches!(state, TagTargetState::Written { .. }),
                    |row| {
                        row.child(
                            Button::new(SharedString::from(format!("tags-restore-{id}-{file}")))
                                .ghost()
                                .label("Restore original")
                                .accessibility_label(format!("Restore original {name}"))
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.send(Command::Restore(id, file), cx)
                                })),
                        )
                    },
                )
                .into_any_element()
        }))
        .children(
            record
                .sidecars
                .iter()
                .enumerate()
                .map(|(i, (sidecar, state))| {
                    let text = format!(
                        "{}: {}",
                        sidecar.directory.join(&sidecar.file_name).display(),
                        match state {
                            TagSidecarState::Pending => "Not written".to_owned(),
                            TagSidecarState::Written => "Saved".to_owned(),
                            TagSidecarState::Changed => {
                                "Folder changed since review; review again".to_owned()
                            }
                            TagSidecarState::Failed { reason } => format!("Failed: {reason}"),
                        }
                    );
                    label(format!("tags-sidecar-outcome-{id}-{i}"), text)
                        .text_sm()
                        .into_any_element()
                }),
        )
        .into_any_element()
}

impl Render for FilesAndTags {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let review = self.status.review.clone();
        let mut page = v_flex()
            .id("files-and-tags")
            .test_support()
            .gap_6()
            .child(self.render_status(cx));
        match review {
            None if self.album.is_none() => {
                page = page.child(label(
                    "tags-no-album",
                    "Open an album and choose Files & tags.".into(),
                ));
            }
            None => {}
            Some(review) => {
                let (title, subtitle) = album_heading(&review);
                page = page
                    .child(
                        h_flex()
                            .gap_4()
                            .items_center()
                            .flex_wrap()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(label("tags-album-title", title).text_xl())
                                    .child(
                                        label("tags-album-subtitle", subtitle)
                                            .text_color(cx.theme().muted_foreground),
                                    ),
                            )
                            .child(
                                Button::new("tags-reload")
                                    .label("Reload files")
                                    .disabled(self.status.busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(album) = this.album {
                                            this.plan = None;
                                            this.send(Command::Load(album), cx);
                                        }
                                    })),
                            ),
                    )
                    .child(self.render_fields(&review, window, cx))
                    .child(self.render_tracks(&review, window, cx))
                    .child(self.render_artwork(&review, cx))
                    .child(self.render_review(cx))
                    .child(self.render_history(cx));
            }
        }
        page
    }
}

fn default_art() -> Vec<ArtSetting> {
    PICTURE_TYPES
        .iter()
        .map(|_| ArtSetting {
            destination: ArtworkDestination::Embed,
            replace_embedded: false,
            replace_sidecars: false,
        })
        .collect()
}

fn album_heading(review: &AlbumReview) -> (String, String) {
    let first = |field: &TagField| {
        review
            .current_values(field)
            .into_iter()
            .find(|v| !v.is_empty())
            .map(|v| v.join("; "))
    };
    let readable = review
        .files
        .iter()
        .filter(|f| matches!(f.state, FileState::Ready { .. }))
        .count();
    (
        first(&TagField::Album).unwrap_or_else(|| "Untitled album".into()),
        format!(
            "{} · {} files, {readable} writable",
            first(&TagField::AlbumArtist)
                .or_else(|| first(&TagField::Artist))
                .unwrap_or_else(|| "Unknown artist".into()),
            review.files.len()
        ),
    )
}

fn section(id: &'static str, title: &'static str, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    v_flex().id(id).gap_2().child(
        div()
            .id(SharedString::from(format!("{id}-heading")))
            .role(Role::Heading)
            .aria_level(2)
            .aria_label(title)
            .text_size(px(22.))
            .font_semibold()
            .text_color(cx.theme().foreground)
            .child(title),
    )
}

fn label(id: impl Into<SharedString>, text: String) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(gpui_kit::ElementId::Name(id.into()))
        .role(Role::Label)
        .aria_value(text.clone())
        .child(text)
}

fn field_key(field: &TagField) -> String {
    field_label(field).to_lowercase().replace(' ', "-")
}

fn track_key(file: MediaFileId, field: &TagField) -> String {
    format!("track-{file}-{}", field_key(field))
}

fn picture_key(kind: PictureType) -> &'static str {
    match kind {
        PictureType::FrontCover => "front",
        PictureType::BackCover => "back",
        PictureType::Booklet => "booklet",
        PictureType::Other => "other",
    }
}

fn multi_hint(field: &TagField) -> &'static str {
    match field {
        TagField::Genre | TagField::Style | TagField::Label | TagField::CatalogNumber => {
            "Separate values with ;"
        }
        _ => "New value",
    }
}

fn job_label(state: &JobState) -> &'static str {
    match state {
        JobState::Queued => "Queued",
        JobState::Running(_) => "Writing",
        JobState::Completed => "Complete",
        JobState::Incomplete => "Partial",
        JobState::Failed { .. } => "Failed",
        JobState::Stale => "Stale",
        JobState::Cancelled => "Cancelled",
    }
}

fn target_label(state: &TagTargetState) -> String {
    match state {
        TagTargetState::Pending => "Not written".into(),
        TagTargetState::Replacing { .. } => "Interrupted".into(),
        TagTargetState::Written { .. } => "Written".into(),
        TagTargetState::Unchanged => "Already up to date".into(),
        TagTargetState::Changed => "Changed since review; review again".into(),
        TagTargetState::Deferred { reason } => format!("Deferred: {reason}"),
        TagTargetState::Failed { reason } => format!("Failed: {reason}"),
        TagTargetState::Restored { .. } => "Original restored".into(),
    }
}

fn write_time(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_default()
}
