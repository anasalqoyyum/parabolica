pub mod remote;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::controllers::queue::EnqueueMode;
use crate::controllers::queue::{QueueService, QueueSnapshot, SourceView};
use crate::ui::queue::{error_label, source_label};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Selectable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Entity, PathPromptOptions, Render, Role, SharedString, Task,
    TestSupportExt as _, Window, div, px, uniform_list,
};
use parabolica_domain::ids::PlaylistId;

pub struct PlaylistsView {
    queue: QueueService,
    state: Arc<QueueSnapshot>,
    selected: Option<PlaylistId>,
    name: Entity<InputState>,
    confirm_delete: bool,
    last_created: Option<PlaylistId>,
    _poll: Task<()>,
}

impl PlaylistsView {
    pub fn new(queue: QueueService, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = queue.snapshot();
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Playlist name"));
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        let state = this.queue.snapshot();
                        if state.revision != this.state.revision {
                            if state.last_created_playlist != this.last_created {
                                this.last_created = state.last_created_playlist;
                                this.selected = this.last_created;
                            }
                            if this.selected.is_some_and(|id| {
                                !state.playlists.iter().any(|playlist| playlist.id == id)
                            }) {
                                this.selected = None;
                                this.confirm_delete = false;
                            }
                            this.state = state;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            queue,
            state,
            selected: None,
            name,
            confirm_delete: false,
            last_created: None,
            _poll: poll,
        }
    }

    fn playlist(&self) -> Option<&crate::controllers::queue::PlaylistView> {
        self.selected.and_then(|id| {
            self.state
                .playlists
                .iter()
                .find(|playlist| playlist.id == id)
        })
    }

    fn source_count(&self) -> usize {
        self.playlist()
            .map_or(self.state.likes.len(), |playlist| playlist.entries.len())
    }

    fn source_at(&self, index: usize) -> &SourceView {
        self.playlist().map_or_else(
            || &self.state.likes[index],
            |playlist| &playlist.entries[index].details,
        )
    }

    fn play(&self, mode: EnqueueMode) {
        if let Some(id) = self.selected {
            self.queue.play_playlist(id, mode);
        } else {
            self.queue.enqueue_sources(
                self.state
                    .likes
                    .iter()
                    .map(|entry| entry.source.clone())
                    .collect(),
                mode,
            );
        }
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import M3U8".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                let name = path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Imported playlist")
                    .to_owned();
                let _ = this.update(cx, |this, _| {
                    this.queue.import_m3u8(path, name);
                });
            }
        })
        .detach();
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        let directory = (0..self.source_count())
            .find_map(|index| {
                self.source_at(index)
                    .path
                    .as_ref()
                    .and_then(|path| path.parent())
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let path = cx.prompt_for_new_path(&directory, Some("playlist.m3u8"));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = path.await {
                let _ = this.update(cx, |this, _| {
                    this.queue.export_m3u8(id, path);
                });
            }
        })
        .detach();
    }

    fn row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let source = self.source_at(index).clone();
        let play_source = source.source.clone();
        let like_source = source.source.clone();
        let liked = self
            .state
            .likes
            .iter()
            .any(|like| like.source == source.source);
        let mut row = h_flex()
            .id(SharedString::from(format!("playlist-row-{index}")))
            .test_support()
            .w_full()
            .h(px(84.))
            .gap_1()
            .min_w_0()
            .child(source_label(&source, cx))
            .child(
                Button::new(SharedString::from(format!("playlist-play-{index}")))
                    .ghost()
                    .icon(IconName::Play)
                    .accessibility_label("Play now")
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.queue
                            .enqueue_sources(vec![play_source.clone()], EnqueueMode::Now);
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("playlist-like-{index}")))
                    .ghost()
                    .icon(IconName::Heart)
                    .selected(liked)
                    .accessibility_label(if liked {
                        "Unlike in app"
                    } else {
                        "Like in app"
                    })
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.queue.set_local_like(like_source.clone(), !liked);
                    })),
            );
        if let Some(id) = self.selected
            && let Some(playlist) = self
                .state
                .playlists
                .iter()
                .find(|playlist| playlist.id == id)
        {
            let entry_id = playlist.entries[index].id;
            let order: Vec<_> = playlist.entries.iter().map(|entry| entry.id).collect();
            let mut up = order.clone();
            if index > 0 {
                up.swap(index, index - 1);
            }
            let mut down = order;
            if index + 1 < down.len() {
                down.swap(index, index + 1);
            }
            row = row
                .child(
                    Button::new(SharedString::from(format!("playlist-up-{index}")))
                        .ghost()
                        .icon(IconName::ChevronUp)
                        .accessibility_label("Move up")
                        .disabled(index == 0)
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.queue.reorder_playlist(id, up.clone());
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("playlist-down-{index}")))
                        .ghost()
                        .icon(IconName::ChevronDown)
                        .accessibility_label("Move down")
                        .disabled(index + 1 == playlist.entries.len())
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.queue.reorder_playlist(id, down.clone());
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("playlist-remove-{index}")))
                        .ghost()
                        .icon(IconName::X)
                        .accessibility_label("Remove from playlist")
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.queue.remove_playlist_entry(id, entry_id);
                        })),
                );
        }
        row.into_any_element()
    }
}

impl Render for PlaylistsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.source_count();
        let selected = self.selected;
        v_flex()
            .id("playlists-page")
            .test_support()
            .size_full()
            .min_h_0()
            .gap_3()
            .child(
                h_flex()
                    .id("playlist-navigation")
                    .role(Role::List)
                    .aria_label("Playlists")
                    .gap_2()
                    .flex_wrap()
                    .child(crate::ui::shell::NavItem::new(
                        "playlist-likes",
                        "Liked in app",
                        selected.is_none(),
                        cx.listener(|this, _, _, cx| {
                            this.selected = None;
                            this.confirm_delete = false;
                            cx.notify();
                        }),
                    ))
                    .children(self.state.playlists.iter().map(|playlist| {
                        let id = playlist.id;
                        crate::ui::shell::NavItem::new(
                            SharedString::from(format!("playlist-{id}")),
                            playlist.name.clone(),
                            selected == Some(id),
                            cx.listener(move |this, _, window, cx| {
                                this.selected = Some(id);
                                this.confirm_delete = false;
                                if let Some(playlist) = this
                                    .state
                                    .playlists
                                    .iter()
                                    .find(|playlist| playlist.id == id)
                                {
                                    let name = playlist.name.clone();
                                    this.name
                                        .update(cx, |input, cx| input.set_value(name, window, cx));
                                }
                                cx.notify();
                            }),
                        )
                    })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Input::new(&self.name)
                            .id("playlist-name")
                            .aria_label("Playlist name")
                            .w(px(200.)),
                    )
                    .child(
                        Button::new("playlist-create")
                            .label("Create playlist")
                            .disabled(!self.state.storage_available)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.queue
                                    .create_playlist(this.name.read(cx).value().to_string());
                            })),
                    )
                    .child(
                        Button::new("playlist-import")
                            .label("Import M3U8")
                            .disabled(!self.state.storage_available)
                            .on_click(cx.listener(|this, _, _, cx| this.import(cx))),
                    )
                    .when_some(selected, |row, id| {
                        row.child(Button::new("playlist-rename").label("Rename").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.queue
                                    .rename_playlist(id, this.name.read(cx).value().to_string());
                            }),
                        ))
                        .child(
                            Button::new("playlist-delete")
                                .ghost()
                                .label(if self.confirm_delete {
                                    "Confirm delete playlist"
                                } else {
                                    "Delete playlist"
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if this.confirm_delete {
                                        this.queue.delete_playlist(id);
                                        this.confirm_delete = false;
                                    } else {
                                        this.confirm_delete = true;
                                    }
                                    cx.notify();
                                })),
                        )
                        .when(self.confirm_delete, |row| {
                            row.child(
                                Button::new("playlist-cancel-delete")
                                    .ghost()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_delete = false;
                                        cx.notify();
                                    })),
                            )
                        })
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("playlist-play-all")
                            .primary()
                            .label("Play now")
                            .disabled(count == 0)
                            .on_click(cx.listener(|this, _, _, _| this.play(EnqueueMode::Now))),
                    )
                    .child(
                        Button::new("playlist-next")
                            .label("Play next")
                            .disabled(count == 0)
                            .on_click(cx.listener(|this, _, _, _| this.play(EnqueueMode::Next))),
                    )
                    .child(
                        Button::new("playlist-later")
                            .label("Play later")
                            .disabled(count == 0)
                            .on_click(cx.listener(|this, _, _, _| this.play(EnqueueMode::Later))),
                    )
                    .when_some(selected, |row, id| {
                        row.child(
                            Button::new("playlist-add-queue")
                                .label("Add queue")
                                .disabled(self.state.entries.is_empty())
                                .on_click(cx.listener(move |this, _, _, _| {
                                    this.queue.append_playlist(
                                        id,
                                        this.state
                                            .entries
                                            .iter()
                                            .map(|entry| entry.entry.source.clone())
                                            .collect(),
                                    );
                                })),
                        )
                        .child(
                            Button::new("playlist-export")
                                .label("Export M3U8")
                                .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
                        )
                    }),
            )
            .child(error_label(self.state.error.as_deref(), cx))
            .when(self.state.error.is_some(), |view| {
                view.child(
                    Button::new("playlist-dismiss-error")
                        .ghost()
                        .label("Dismiss error")
                        .on_click(cx.listener(|this, _, _, _| this.queue.dismiss_error())),
                )
            })
            .when_some(self.state.last_export.as_ref(), |column, export| {
                column.children(export.non_exportable.iter().map(|problem| {
                    div()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(format!("{}: {}", problem.entry, problem.reason))
                }))
            })
            .when(count == 0, |view| {
                view.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("No songs"),
                )
            })
            .child(
                uniform_list(
                    "playlist-entries",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range.map(|index| this.row(index, cx)).collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .min_h_0(),
            )
    }
}

#[cfg(test)]
mod tests;
