//! Local collection pages backed by stored metadata and FTS search.

pub mod remote;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, Global, Image, ImageFormat, PathPromptOptions, Render, Role,
    SharedString, Task, TestSupportExt as _, Window, div, img, px, uniform_list,
};
use parabolica_domain::control::CancelToken;
use parabolica_domain::ids::{MediaFileId, ReleaseId};
use parabolica_library::artwork::ThumbnailCache;
use parabolica_library::service::{Command, LibraryService, LibraryStatus};
use parabolica_storage::{
    Storage,
    collection::{BrowseKind, BrowseQuery, CollectionItem, ItemId, Sort},
    library::LibraryRoot,
};

use crate::controllers::queue::EnqueueMode;
use crate::ui::player::Player;
use parabolica_domain::source::PlayableSource;

const PAGE_SIZE: usize = 96;
struct Library {
    storage: Option<Storage>,
    service: Option<Arc<LibraryService>>,
}
impl Global for Library {}
pub fn init(storage: Option<Storage>, cx: &mut App) {
    let service = storage.clone().map(LibraryService::start).map(Arc::new);
    cx.set_global(Library { storage, service });
}

pub(crate) fn storage(cx: &App) -> Option<Storage> {
    cx.try_global::<Library>()
        .and_then(|library| library.storage.clone())
}

pub struct LibraryView {
    pub(crate) storage: Option<Storage>,
    pub(crate) service: Option<Arc<LibraryService>>,
    player: Entity<Player>,
    pub(crate) roots: Vec<LibraryRoot>,
    kind: BrowseKind,
    pub(crate) folders_settings: bool,
    query: BrowseQuery,
    items: Vec<CollectionItem>,
    selection: Option<CollectionItem>,
    checked: HashSet<MediaFileId>,
    grouping_target: Option<(ReleaseId, String)>,
    pub(crate) confirm_remove: Option<parabolica_domain::ids::LibraryRootId>,
    pub(crate) status: LibraryStatus,
    pub(crate) error: Option<String>,
    loading: bool,
    more: bool,
    generation: u64,
    revision: u64,
    art: HashMap<MediaFileId, Arc<Image>>,
    art_cancel: CancelToken,
    request: Option<Task<()>>,
    artwork: Option<Task<()>>,
    playback_request: Option<Task<()>>,
    _poll: Task<()>,
}
impl LibraryView {
    pub fn new(player: Entity<Player>, cx: &mut Context<Self>) -> Self {
        let library = cx.try_global::<Library>();
        let storage = library.and_then(|l| l.storage.clone());
        let service = library.and_then(|l| l.service.clone());
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        let Some(service) = &this.service else {
                            return;
                        };
                        service.set_playing(this.player.read(cx).is_playing());
                        let status = service.status();
                        if status.revision != this.revision {
                            this.revision = status.revision;
                            if this.status.scanning && !status.scanning {
                                this.art.clear();
                            }
                            this.status = status;
                            this.refresh(false, cx);
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut this = Self {
            storage,
            service,
            player,
            roots: Vec::new(),
            kind: BrowseKind::Albums,
            folders_settings: false,
            query: BrowseQuery {
                limit: PAGE_SIZE + 1,
                ..Default::default()
            },
            items: Vec::new(),
            selection: None,
            checked: HashSet::new(),
            grouping_target: None,
            confirm_remove: None,
            status: LibraryStatus::default(),
            error: None,
            loading: false,
            more: false,
            generation: 0,
            revision: 0,
            art: HashMap::new(),
            art_cancel: CancelToken::new(),
            request: None,
            artwork: None,
            playback_request: None,
            _poll: poll,
        };
        this.refresh(false, cx);
        this
    }
    pub fn show(&mut self, kind: BrowseKind, settings: bool, cx: &mut Context<Self>) {
        self.kind = kind;
        self.folders_settings = settings;
        self.selection = None;
        self.query.album = None;
        self.query.root = None;
        self.query.artist = None;
        self.query.folder = None;
        self.query.offset = 0;

        self.refresh(false, cx);
    }
    pub fn search(&mut self, text: String, cx: &mut Context<Self>) {
        if self.query.text == text {
            return;
        }
        self.query.text = text;
        self.query.offset = 0;
        self.refresh(true, cx);
    }
    fn refresh(&mut self, debounce: bool, cx: &mut Context<Self>) {
        let Some(storage) = self.storage.clone() else {
            return;
        };
        self.generation += 1;
        let generation = self.generation;
        let kind = self.kind;
        let query = self.query.clone();
        self.loading = true;
        self.request = Some(cx.spawn(async move |this, cx| {
            if debounce {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
            }
            let result = cx
                .background_spawn(async move {
                    let roots = storage.library_roots()?;
                    let items = storage.browse_collection(kind, query)?;
                    Ok::<_, parabolica_storage::StorageError>((roots, items))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                this.loading = false;
                match result {
                    Ok((roots, mut items)) => {
                        this.roots = roots;
                        this.more = items.len() > PAGE_SIZE;
                        items.truncate(PAGE_SIZE);
                        this.items = items;
                        this.load_artwork(cx);
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn load_artwork(&mut self, cx: &mut Context<Self>) {
        self.art_cancel.cancel();
        self.art_cancel = CancelToken::new();
        if self.kind != BrowseKind::Albums || self.folders_settings {
            return;
        }
        let Some(storage) = self.storage.clone() else {
            return;
        };
        let files = self
            .items
            .iter()
            .filter_map(|item| item.artwork_file)
            .filter(|file| !self.art.contains_key(file))
            .collect::<Vec<_>>();
        let keep = self
            .items
            .iter()
            .filter_map(|item| item.artwork_file)
            .collect::<HashSet<_>>();
        self.art.retain(|file, _| keep.contains(file));
        let cancel = self.art_cancel.clone();
        let generation = self.generation;
        self.artwork = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_spawn(async move {
                    let mut cache = ThumbnailCache::default();
                    let mut images = Vec::new();
                    for file in files {
                        if cancel.is_cancelled() {
                            break;
                        }
                        let Ok(Some(record)) = storage.media_file(file) else {
                            continue;
                        };
                        let Ok(path) = storage.resolve_local_file(file) else {
                            continue;
                        };
                        if let Ok(Some(bytes)) = cache.load(file, record.signature, &path, &cancel)
                        {
                            images.push((
                                file,
                                Arc::new(Image::from_bytes(
                                    ImageFormat::Png,
                                    bytes.as_ref().clone(),
                                )),
                            ));
                        }
                    }
                    images
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.art.extend(loaded);
                    cx.notify();
                }
            });
        }));
    }
    pub(crate) fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        self.error = self
            .service
            .as_ref()
            .ok_or_else(|| "Library database unavailable".to_owned())
            .and_then(|service| service.send(command))
            .err();
        cx.notify();
    }
    pub(crate) fn prompt_folder(
        &mut self,
        root: Option<parabolica_domain::ids::LibraryRootId>,
        cx: &mut Context<Self>,
    ) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(
                if root.is_some() {
                    "Locate folder"
                } else {
                    "Add music folder"
                }
                .into(),
            ),
        });
        cx.spawn(async move |this, cx| match prompt.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = this.update(cx, |this, cx| {
                        this.send(
                            root.map_or_else(
                                || Command::Add(path.clone()),
                                |root| Command::Relocate(root, path.clone()),
                            ),
                            cx,
                        )
                    });
                }
            }
            Ok(Err(error)) => {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(error.to_string());
                    cx.notify();
                });
            }
            _ => {}
        })
        .detach();
    }
    pub(crate) fn browse_root(
        &mut self,
        root: parabolica_domain::ids::LibraryRootId,
        cx: &mut Context<Self>,
    ) {
        self.folders_settings = false;
        self.kind = BrowseKind::Folders;
        self.selection = None;
        self.query = BrowseQuery {
            root: Some(root),
            limit: PAGE_SIZE + 1,
            ..Default::default()
        };
        self.refresh(false, cx);
    }
    fn open_item(&mut self, item: CollectionItem, cx: &mut Context<Self>) {
        match item.id.clone() {
            ItemId::Album(album) => {
                self.query.album = Some(album);
                self.query.sort = Sort::TrackOrder;
                self.grouping_target = Some((album, item.title.clone()));
            }
            ItemId::Artist(artist) => self.query.artist = Some(artist),
            ItemId::Folder(root, folder) => {
                self.query.root = Some(root);
                self.query.folder = Some(folder);
            }
            ItemId::Song(file) => {
                self.play(Some(file), cx);
                return;
            }
        }
        self.selection = Some(item);
        self.kind = BrowseKind::Songs;
        self.query.offset = 0;

        self.refresh(false, cx);
    }
    fn play(&mut self, file: Option<MediaFileId>, cx: &mut Context<Self>) {
        self.enqueue(file, EnqueueMode::Now, cx);
    }

    fn enqueue(&mut self, file: Option<MediaFileId>, mode: EnqueueMode, cx: &mut Context<Self>) {
        let Some(storage) = self.storage.clone() else {
            return;
        };
        let mut query = if file.is_none() && self.query.album.is_some() {
            BrowseQuery {
                album: self.query.album,
                ..Default::default()
            }
        } else {
            self.query.clone()
        };
        query.offset = 0;
        query.limit = 256;
        query.sort = Sort::TrackOrder;
        let player = self.player.clone();
        self.playback_request = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let mut sources = Vec::new();
                    if let Some(file) = file {
                        sources.push(PlayableSource::LocalFile(file));
                    } else {
                        loop {
                            let rows =
                                storage.browse_collection(BrowseKind::Songs, query.clone())?;
                            let count = rows.len();
                            for item in rows {
                                if let ItemId::Song(file) = item.id {
                                    sources.push(PlayableSource::LocalFile(file));
                                }
                            }
                            if count < 256 {
                                break;
                            }
                            query.offset += count;
                        }
                    }
                    Ok::<_, parabolica_storage::StorageError>(sources)
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(sources) if !sources.is_empty() => {
                    player.read(cx).enqueue(sources, mode);
                }
                Ok(_) => {
                    this.error = Some("No songs in this selection".into());
                    cx.notify();
                }
                Err(error) => {
                    this.error = Some(format!("Can't queue selection: {error}"));
                    cx.notify();
                }
            });
        }));
    }

    fn selected_sources(&self) -> Vec<PlayableSource> {
        let mut files: Vec<_> = self.checked.iter().copied().collect();
        files.sort_by_key(|id| id.to_bytes());
        files.into_iter().map(PlayableSource::LocalFile).collect()
    }
    fn recover(&mut self, file: MediaFileId, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Locate file".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = prompt.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| this.send(Command::Recover(file, path), cx));
            }
        })
        .detach();
    }
    fn render_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let has_scope = self.selection.is_some() || self.query.root.is_some();
        h_flex()
            .flex_wrap()
            .gap_2()
            .when(has_scope, |row| {
                row.child(
                    Button::new("collection-back")
                        .label("All music")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.query = BrowseQuery {
                                limit: PAGE_SIZE + 1,
                                ..Default::default()
                            };
                            this.selection = None;
                            this.kind = BrowseKind::Albums;

                            this.refresh(false, cx);
                        })),
                )
            })
            .child(
                Button::new("add-music-folder")
                    .label("Add music folder")
                    .disabled(self.storage.is_none())
                    .icon(IconName::Plus)
                    .on_click(cx.listener(|this, _, _, cx| this.prompt_folder(None, cx))),
            )
            .child(
                Button::new("collection-folders")
                    .label("Folders")
                    .selected(self.kind == BrowseKind::Folders)
                    .on_click(
                        cx.listener(|this, _, _, cx| this.show(BrowseKind::Folders, false, cx)),
                    ),
            )
            .child(
                Button::new("collection-available")
                    .label(if self.query.available_only {
                        "Available files"
                    } else {
                        "Local files"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.query.available_only = !this.query.available_only;
                        this.query.offset = 0;
                        this.refresh(false, cx);
                    })),
            )
            .child(
                Button::new("collection-sort")
                    .label(match self.query.sort {
                        Sort::Title => "Sort: title",
                        Sort::Artist => "Sort: artist",
                        Sort::RecentlyAdded => "Sort: recent",
                        Sort::TrackOrder => "Sort: disc / track",
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.query.sort = match this.query.sort {
                            Sort::Title => Sort::Artist,
                            Sort::Artist => Sort::RecentlyAdded,
                            Sort::RecentlyAdded => Sort::TrackOrder,
                            Sort::TrackOrder => Sort::Title,
                        };
                        this.query.offset = 0;
                        this.refresh(false, cx);
                    })),
            )
            .child(
                Button::new("collection-rescan")
                    .label("Rescan")
                    .on_click(cx.listener(|this, _, _, cx| this.send(Command::Rescan, cx))),
            )
            .when(self.status.scanning, |row| {
                row.child(
                    Button::new("collection-cancel")
                        .label("Cancel indexing")
                        .on_click(cx.listener(|this, _, _, _| {
                            if let Some(service) = &this.service {
                                service.cancel();
                            }
                        })),
                )
            })
            .into_any_element()
    }
    pub(crate) fn render_index_failures(&self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .id("index-failures")
            .test_support()
            .gap_2()
            .child(div().font_medium().child("Indexing errors"))
            .child(
                div()
                    .id("index-failure-details")
                    .test_support()
                    .role(Role::Alert)
                    .aria_label(self.status.failures.join("\n"))
                    .max_h(px(160.))
                    .overflow_y_scroll()
                    .children(self.status.failures.iter().map(|failure| {
                        div()
                            .text_sm()
                            .text_color(cx.theme().warning)
                            .child(failure.clone())
                    })),
            )
            .child(
                Button::new("index-failures-rescan")
                    .label("Rescan")
                    .on_click(cx.listener(|this, _, _, cx| this.send(Command::Rescan, cx))),
            )
            .into_any_element()
    }

    fn render_grouping(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap_2()
            .flex_wrap()
            .child(label(
                "collection-selected",
                format!("{} selected", self.checked.len()),
                cx,
            ))
            .child(
                Button::new("selection-next")
                    .label("Play next")
                    .disabled(self.checked.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.player
                            .read(cx)
                            .enqueue(this.selected_sources(), EnqueueMode::Next)
                    })),
            )
            .child(
                Button::new("selection-later")
                    .label("Play later")
                    .disabled(self.checked.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.player
                            .read(cx)
                            .enqueue(this.selected_sources(), EnqueueMode::Later)
                    })),
            )
            .child(
                Button::new("selection-like")
                    .label("Like in app")
                    .disabled(self.checked.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        for source in this.selected_sources() {
                            this.player.read(cx).queue().set_local_like(source, true);
                        }
                    })),
            )
            .when_some(self.grouping_target.clone(), |row, (album, title)| {
                row.child(
                    Button::new("group-selected")
                        .label(format!("Group into {title}"))
                        .disabled(self.checked.is_empty())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.send(
                                Command::Group(this.checked.iter().copied().collect(), album),
                                cx,
                            )
                        })),
                )
            })
            .child(
                Button::new("split-grouping")
                    .label("Create album group")
                    .disabled(self.checked.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send(
                            Command::SplitGroup(this.checked.iter().copied().collect()),
                            cx,
                        )
                    })),
            )
            .child(
                Button::new("clear-selected")
                    .label("Clear selection")
                    .disabled(self.checked.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.checked.clear();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("reset-grouping")
                    .label("Reset grouping")
                    .disabled(self.checked.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send(
                            Command::ResetGroup(this.checked.iter().copied().collect()),
                            cx,
                        )
                    })),
            )
            .into_any_element()
    }
    fn render_item(
        &self,
        item: &CollectionItem,
        index: usize,
        grid: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = item.clone();
        let available = item.available;
        let summary = format!(
            "{} · {} · {}{}",
            item.title,
            item.subtitle,
            item.detail,
            if available { "" } else { " · Unavailable" }
        );
        let mut content = v_flex().gap_1().min_w_0();
        if grid {
            content = content.child(
                match item.artwork_file.and_then(|file| self.art.get(&file)) {
                    Some(image) => img(image.clone())
                        .w_full()
                        .h(px(150.))
                        .object_fit(gpui_kit::ObjectFit::Contain)
                        .into_any_element(),
                    None => div()
                        .h(px(150.))
                        .w_full()
                        .bg(cx.theme().muted)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(gpui_kit::component::Icon::new(IconName::Disc3).size(px(40.)))
                        .into_any_element(),
                },
            );
        }
        content = content
            .child(div().truncate().font_medium().child(item.title.clone()))
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(item.subtitle.clone()),
            )
            .child(
                div()
                    .truncate()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if available {
                        item.detail.clone()
                    } else {
                        format!("Unavailable · {}", item.detail)
                    }),
            );
        let body = Button::new(SharedString::from(format!("collection-item-{index}")))
            .ghost()
            .flex_1()
            .min_w_0()
            .h(if grid { px(228.) } else { px(72.) })
            .accessibility_label(summary)
            .child(content)
            .on_click(cx.listener(move |this, _, _, cx| this.open_item(target.clone(), cx)));
        let file = match item.id {
            ItemId::Song(file) => Some(file),
            _ => None,
        };
        h_flex()
            .items_center()
            .min_w_0()
            .flex_1()
            .child(body)
            .when_some(file, |row, file| {
                row.child(
                    Button::new(SharedString::from(format!("select-song-{index}")))
                        .ghost()
                        .label(if self.checked.contains(&file) {
                            "Selected"
                        } else {
                            "Select"
                        })
                        .selected(self.checked.contains(&file))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.checked.remove(&file) {
                                this.checked.insert(file);
                            }
                            cx.notify();
                        })),
                )
                .when(!available, |row| {
                    row.child(
                        Button::new(SharedString::from(format!("locate-song-{index}")))
                            .label("Locate file")
                            .on_click(cx.listener(move |this, _, _, cx| this.recover(file, cx))),
                    )
                })
            })
            .into_any_element()
    }
}
impl Render for LibraryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.folders_settings {
            return crate::ui::sources::folders::render(self, cx);
        }
        let entity = cx.entity();
        let count = self.items.len();
        let grid = self.kind == BrowseKind::Albums;
        let columns = if grid {
            ((f32::from(window.viewport_size().width) - 320.) / 200.)
                .floor()
                .clamp(1., 5.) as usize
        } else {
            1
        };
        let rows = count.div_ceil(columns);
        v_flex()
            .id("local-collection")
            .test_support()
            .size_full()
            .min_h_0()
            .gap_3()
            .child(self.render_toolbar(cx))
            .when(!self.status.failures.is_empty(), |view| {
                view.child(self.render_index_failures(cx))
            })
            .when(
                self.status.scanning
                    || self.status.progress.cancelled
                    || self.status.progress.errors > 0,
                |view| {
                    view.child(label(
                        "collection-progress",
                        format!(
                            "{} · {} indexed · {} unchanged · {} errors",
                            if self.status.scanning {
                                "Indexing"
                            } else if self.status.progress.cancelled {
                                "Cancelled"
                            } else {
                                "Finished"
                            },
                            self.status.progress.indexed,
                            self.status.progress.unchanged,
                            self.status.progress.errors
                        ),
                        cx,
                    ))
                },
            )
            .when_some(
                self.error.clone().or_else(|| {
                    self.status
                        .command_error
                        .clone()
                        .or_else(|| self.status.error.clone())
                }),
                |view, error| view.child(alert("collection-error", error, cx)),
            )
            .when_some(self.status.watcher_error.clone(), |view, error| {
                view.child(alert("collection-watcher-error", error, cx))
            })
            .when(self.storage.is_none(), |view| {
                view.child(alert(
                    "collection-unavailable",
                    "Library database unavailable".into(),
                    cx,
                ))
            })
            .when_some(self.selection.as_ref(), |view, item| {
                let album = matches!(item.id, ItemId::Album(_));
                view.child(
                    h_flex()
                        .items_start()
                        .gap_4()
                        .when(album, |row| {
                            row.child(
                                match item.artwork_file.and_then(|file| self.art.get(&file)) {
                                    Some(image) => img(image.clone())
                                        .w(px(150.))
                                        .h(px(150.))
                                        .object_fit(gpui_kit::ObjectFit::Contain)
                                        .into_any_element(),
                                    None => div()
                                        .w(px(150.))
                                        .h(px(150.))
                                        .bg(cx.theme().muted)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            gpui_kit::component::Icon::new(IconName::Disc3)
                                                .size(px(48.)),
                                        )
                                        .into_any_element(),
                                },
                            )
                        })
                        .child(
                            v_flex()
                                .gap_3()
                                .min_w_0()
                                .child(div().text_xl().child(item.title.clone()))
                                .child(
                                    div()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(item.subtitle.clone()),
                                )
                                .when(album, |column| {
                                    column
                                        .child(
                                            Button::new("play-album")
                                                .primary()
                                                .label("Play album")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.play(None, cx)
                                                })),
                                        )
                                        .child(
                                            Button::new("album-next").label("Play next").on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    this.enqueue(None, EnqueueMode::Next, cx)
                                                }),
                                            ),
                                        )
                                        .child(
                                            Button::new("album-later")
                                                .label("Play later")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.enqueue(None, EnqueueMode::Later, cx)
                                                })),
                                        )
                                }),
                        ),
                )
            })
            .when(self.kind == BrowseKind::Songs, |view| {
                view.child(self.render_grouping(cx))
            })
            .when(self.loading, |view| {
                view.child(label("collection-loading", "Loading".into(), cx))
            })
            .when(
                count == 0 && !self.loading && self.storage.is_some(),
                |view| {
                    view.child(label(
                        "collection-empty",
                        if self.query.text.is_empty() {
                            "No music indexed"
                        } else {
                            "No matches"
                        }
                        .into(),
                        cx,
                    ))
                },
            )
            .child(
                uniform_list("collection-list", rows, move |range, _, cx| {
                    entity.update(cx, |this, cx| {
                        range
                            .map(|row| {
                                h_flex().w_full().gap_4().children(
                                    (row * columns..((row + 1) * columns).min(this.items.len()))
                                        .map(|index| {
                                            this.render_item(&this.items[index], index, grid, cx)
                                        }),
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .flex_1()
                .min_h(px(80.)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("collection-previous")
                            .label("Previous page")
                            .disabled(self.query.offset == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.query.offset = this.query.offset.saturating_sub(PAGE_SIZE);
                                this.refresh(false, cx);
                            })),
                    )
                    .child(label(
                        "collection-page",
                        format!("Page {}", self.query.offset / PAGE_SIZE + 1),
                        cx,
                    ))
                    .child(
                        Button::new("collection-next")
                            .label("Next page")
                            .disabled(!self.more)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.query.offset += PAGE_SIZE;
                                this.refresh(false, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}
impl Drop for LibraryView {
    fn drop(&mut self) {
        self.art_cancel.cancel();
    }
}
pub(crate) fn label(id: &'static str, text: String, cx: &App) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .role(Role::Label)
        .aria_value(text.clone())
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}
pub(crate) fn alert(id: &'static str, text: String, cx: &App) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .role(Role::Alert)
        .aria_label(text.clone())
        .text_color(cx.theme().warning)
        .child(text)
        .into_any_element()
}
