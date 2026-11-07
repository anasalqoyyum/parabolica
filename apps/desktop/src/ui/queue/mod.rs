use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Render, Role, SharedString, Task, TestSupportExt as _, Window, div, px,
    uniform_list,
};

use crate::controllers::queue::{QueueService, QueueSnapshot, SourceView};
use crate::ui::player::{next_repeat, repeat_label, shuffle_seed};

pub struct QueueView {
    queue: QueueService,
    state: Arc<QueueSnapshot>,
    history: bool,
    _poll: Task<()>,
}

impl QueueView {
    pub fn new(queue: QueueService, cx: &mut Context<Self>) -> Self {
        let state = queue.snapshot();
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        let state = this.queue.snapshot();
                        if state.revision != this.state.revision {
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
            history: false,
            _poll: poll,
        }
    }

    fn row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        if self.history {
            let record = &self.state.history[self.state.history.len() - index - 1];
            let source = record.details.source.clone();
            return h_flex()
                .h(px(76.))
                .gap_2()
                .child(source_label(&record.details, cx))
                .child(
                    Button::new(SharedString::from(format!("history-play-{index}")))
                        .ghost()
                        .label("Play now")
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.queue.enqueue_sources(
                                vec![source.clone()],
                                crate::controllers::queue::EnqueueMode::Now,
                            );
                        })),
                )
                .into_any_element();
        }
        let entry = &self.state.entries[index];
        let id = entry.entry.id;
        let source = entry.entry.source.clone();
        let liked = self.state.likes.iter().any(|like| like.source == source);
        let unavailable = entry.details.unavailable_reason.is_some();
        h_flex()
            .id(SharedString::from(format!("queue-row-{index}")))
            .test_support()
            .w_full()
            .h(px(84.))
            .gap_1()
            .min_w_0()
            .when(self.state.current == Some(id), |row| {
                row.bg(cx.theme().accent)
            })
            .child(source_label(&entry.details, cx))
            .child(
                Button::new(SharedString::from(format!("queue-play-{index}")))
                    .ghost()
                    .icon(IconName::Play)
                    .accessibility_label("Play now")
                    .disabled(unavailable)
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.queue.jump(id);
                    })),
            )
            .when(unavailable, |row| {
                row.child(
                    Button::new(SharedString::from(format!("queue-retry-{index}")))
                        .ghost()
                        .label("Retry")
                        .disabled(entry.retry_attempts >= 3)
                        .on_click(cx.listener(move |this, _, _, _| {
                            this.queue.retry(id);
                        })),
                )
            })
            .child(
                Button::new(SharedString::from(format!("queue-like-{index}")))
                    .ghost()
                    .icon(IconName::Heart)
                    .selected(liked)
                    .accessibility_label(if liked {
                        "Unlike in app"
                    } else {
                        "Like in app"
                    })
                    .disabled(!self.state.storage_available)
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.queue.set_local_like(source.clone(), !liked);
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("queue-up-{index}")))
                    .ghost()
                    .icon(IconName::ChevronUp)
                    .accessibility_label("Move up")
                    .disabled(index == 0)
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.queue.move_entry(id, index.saturating_sub(1));
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("queue-down-{index}")))
                    .ghost()
                    .icon(IconName::ChevronDown)
                    .accessibility_label("Move down")
                    .disabled(index + 1 == self.state.entries.len())
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.queue.move_entry(id, index + 1);
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("queue-remove-{index}")))
                    .ghost()
                    .icon(IconName::X)
                    .accessibility_label("Remove from queue")
                    .on_click(cx.listener(move |this, _, _, _| {
                        this.queue.remove(id);
                    })),
            )
            .into_any_element()
    }
}

pub(crate) fn source_label(source: &SourceView, cx: &gpui_kit::App) -> AnyElement {
    let mut label = format!("{}, {}", source.title, source.source_label);
    if let Some(reason) = &source.unavailable_reason {
        label.push_str(&format!(", Unavailable: {reason}"));
    }
    v_flex()
        .id("queue-source-label")
        .test_support()
        .role(Role::Label)
        .aria_value(label)
        .overflow_hidden()
        .flex_1()
        .min_w_0()
        .gap_1()
        .child(div().truncate().font_medium().child(source.title.clone()))
        .child(
            div()
                .truncate()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(source.source_label.clone()),
        )
        .when_some(source.unavailable_reason.clone(), |column, reason| {
            column.child(
                div()
                    .truncate()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(format!("Unavailable: {reason}")),
            )
        })
        .into_any_element()
}

pub(crate) fn error_label(error: Option<&str>, cx: &gpui_kit::App) -> AnyElement {
    div()
        .id("queue-error")
        .test_support()
        .role(Role::Alert)
        .min_w_0()
        .when_some(error, |view, error| {
            view.aria_label(error.to_owned())
                .text_sm()
                .text_color(cx.theme().danger)
                .child(error.to_owned())
        })
        .into_any_element()
}

impl Render for QueueView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = if self.history {
            self.state.history.len()
        } else {
            self.state.entries.len()
        };
        v_flex()
            .id("queue-page")
            .test_support()
            .size_full()
            .min_h_0()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("queue-upcoming")
                            .label("Queue")
                            .selected(!self.history)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.history = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("queue-history")
                            .label("History")
                            .selected(self.history)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.history = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("queue-shuffle")
                            .label("Shuffle")
                            .selected(self.state.shuffle_seed.is_some())
                            .on_click(cx.listener(|this, _, _, _| {
                                this.queue
                                    .set_shuffle(if this.state.shuffle_seed.is_some() {
                                        None
                                    } else {
                                        Some(shuffle_seed())
                                    });
                            })),
                    )
                    .child(
                        Button::new("queue-repeat")
                            .label(repeat_label(self.state.repeat))
                            .on_click(cx.listener(|this, _, _, _| {
                                this.queue.set_repeat(next_repeat(this.state.repeat));
                            })),
                    )
                    .child(
                        Button::new("queue-skip")
                            .label("Skip")
                            .disabled(self.state.current.is_none())
                            .on_click(cx.listener(|this, _, _, _| {
                                this.queue.skip();
                            })),
                    )
                    .child(
                        Button::new("queue-refresh")
                            .ghost()
                            .label("Refresh availability")
                            .on_click(cx.listener(|this, _, _, _| {
                                this.queue.refresh();
                            })),
                    )
                    .child(
                        Button::new("queue-clear")
                            .ghost()
                            .label("Clear queue")
                            .disabled(self.state.entries.is_empty())
                            .on_click(cx.listener(|this, _, _, _| {
                                this.queue.clear();
                            })),
                    ),
            )
            .child(error_label(self.state.error.as_deref(), cx))
            .when(self.state.error.is_some(), |view| {
                view.child(
                    Button::new("queue-dismiss-error")
                        .ghost()
                        .label("Dismiss error")
                        .on_click(cx.listener(|this, _, _, _| this.queue.dismiss_error())),
                )
            })
            .when(count == 0, |view| {
                view.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(if self.history {
                            "No listening history"
                        } else {
                            "Queue is empty"
                        }),
                )
            })
            .child(
                uniform_list(
                    "queue-entries",
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
pub(crate) mod tests;
