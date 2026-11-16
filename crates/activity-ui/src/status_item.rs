//! Statusbar item for the activity registry: one inline entry (the most
//! important one) plus a popover listing everything that is running.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    canvas, div, percentage, px, relative, Animation, AnimationExt, AnyElement, App, AppContext,
    Bounds, ClickEvent, Context, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement,
    ParentElement, Pixels, Point, Render, SharedString, StatefulInteractiveElement, Styled,
    Transformation, Window,
};
use labonair_panel::{AnyStatusItemHandle, StatusItem, StatusItemRegistration, StatusSide};
use labonair_theme::store::ThemeStore;
use labonair_ui_kit::{
    icon_button_builder, popover_with_exit, ButtonSize, IconName, ListItem, Palette, Presence,
    Tooltip,
};

use crate::{Activity, ActivityCenter, ActivityKind};

const MAX_MESSAGE_CHARS: usize = 50;

fn truncate(message: &str) -> String {
    if message.chars().count() <= MAX_MESSAGE_CHARS {
        return message.to_string();
    }
    let kept: String = message.chars().take(MAX_MESSAGE_CHARS - 1).collect();
    format!("{kept}\u{2026}")
}

fn state_icon(activity: &Activity) -> IconName {
    match activity.draft.kind {
        ActivityKind::Notice => IconName::Info,
        ActivityKind::Failure | ActivityKind::Health => IconName::Warning,
        ActivityKind::Package => IconName::Download,
        _ => IconName::LoadCircle,
    }
}

fn icon_element(activity: &Activity, palette: &Palette, size: Pixels) -> AnyElement {
    let color = match activity.draft.kind {
        ActivityKind::Failure => palette.error,
        ActivityKind::Health => palette.warning,
        _ => palette.muted,
    };
    let svg = state_icon(activity).svg(color).size(size);
    if activity.draft.kind.is_running() && state_icon(activity) == IconName::LoadCircle {
        svg.with_animation(
            SharedString::from(format!("activity-spin-{}", activity.id)),
            Animation::new(Duration::from_secs(2)).repeat(),
            |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
        )
        .into_any_element()
    } else {
        svg.into_any_element()
    }
}

fn inline_label(primary: &Activity, extra: usize) -> String {
    let mut label = truncate(&primary.draft.message);
    if let Some(progress) = primary.draft.progress {
        label.push_str(&format!(" ({}%)", (progress * 100.0).round() as u32));
    }
    if extra > 0 {
        label.push_str(&format!(" + {extra} more"));
    }
    label
}

pub struct ActivityStatusItem {
    center: Entity<ActivityCenter>,
    theme: Entity<ThemeStore>,
    presence: Presence,
    trigger_bounds: Option<Bounds<Pixels>>,
    fallback_anchor: Point<Pixels>,
    focus: FocusHandle,
}

impl ActivityStatusItem {
    pub fn new(
        center: Entity<ActivityCenter>,
        theme: Entity<ThemeStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&center, |_, _, cx| cx.notify()).detach();
        cx.observe(&theme, |_, _, cx| cx.notify()).detach();
        Self {
            center,
            theme,
            presence: Presence::new(),
            trigger_bounds: None,
            fallback_anchor: Point::default(),
            focus: cx.focus_handle(),
        }
    }

    fn is_open(&self) -> bool {
        self.presence.is_mounted() && !self.presence.is_closing()
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        let motion = Palette::from_theme(self.theme.read(cx)).motion;
        self.presence.request_close(Instant::now(), &motion);
        cx.notify();
    }

    fn render_popover_rows(
        &self,
        entries: Vec<Activity>,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (fg, muted, border) = (palette.fg, palette.muted, palette.border);
        div()
            .id("activity-list")
            .max_h(px(320.0))
            .overflow_y_scroll()
            .children(entries.into_iter().map(|entry| {
                let id = entry.id;
                let trailing = if entry.draft.cancel_action.is_some() {
                    Some(("Cancel", true))
                } else if entry.draft.kind == ActivityKind::Failure {
                    Some(("Dismiss", false))
                } else {
                    None
                };
                let mut body = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_xs()
                            .text_color(fg)
                            .truncate()
                            .child(SharedString::from(entry.draft.message.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(muted)
                            .child(SharedString::from(entry.draft.source.clone())),
                    );
                if let Some(progress) = entry.draft.progress {
                    body = body.child(
                        div()
                            .h(px(3.0))
                            .w_full()
                            .rounded_full()
                            .bg(palette.border)
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(progress))
                                    .rounded_full()
                                    .bg(palette.primary),
                            ),
                    );
                }
                let mut row = ListItem::new(SharedString::from(format!("activity-row-{id}")), palette)
                    .extra(move |row| row.border_b_1().border_color(border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(icon_element(&entry, &palette, px(13.0)))
                            .child(body),
                    );
                if let Some((label, is_cancel)) = trailing {
                    row = row.trailing(
                        icon_button_builder(
                            SharedString::from(format!("activity-x-{id}")),
                            palette,
                            IconName::X,
                        )
                        .size(ButtonSize::IconXxs)
                        .tooltip(label)
                        .render()
                        .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                            cx.stop_propagation();
                            this.center.update(cx, |center, cx| {
                                if is_cancel {
                                    center.activate(id, true, cx);
                                } else {
                                    center.dismiss(id, cx);
                                }
                            });
                        })),
                    );
                }
                row
            }))
            .into_any_element()
    }
}

impl Focusable for ActivityStatusItem {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ActivityStatusItem {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_status(window, cx)
    }
}

impl StatusItem for ActivityStatusItem {
    fn id(&self) -> &'static str {
        "activity"
    }

    fn default_side(&self) -> StatusSide {
        StatusSide::Left
    }

    fn order(&self) -> i32 {
        100
    }

    fn group(&self) -> u32 {
        3
    }

    fn is_empty(&self, cx: &App) -> bool {
        self.center.read(cx).snapshot().is_empty()
    }

    fn render_status(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let entries = self.center.read(cx).snapshot();
        let Some(primary) = entries.first().cloned() else {
            // Nothing running: drop a leftover open popover instead of
            // leaving an empty card behind.
            self.presence = Presence::new();
            return div().into_any_element();
        };
        let palette = Palette::from_theme(self.theme.read(cx));
        self.presence.tick(Instant::now(), &palette.motion);

        let label = inline_label(&primary, entries.len() - 1);
        let tooltip = primary
            .draft
            .tooltip
            .clone()
            .or_else(|| (label != primary.draft.message).then(|| primary.draft.message.clone()));
        let has_click = primary.draft.click_action.is_some();
        let primary_id = primary.id;

        let trigger = div()
            .id("activity-trigger")
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .h_full()
            .rounded_sm()
            .cursor_pointer()
            .text_size(px(11.0))
            .text_color(palette.muted)
            .track_focus(&self.focus)
            .key_context("StatusPopover")
            .relative()
            .child(icon_element(&primary, &palette, px(12.0)))
            .child(SharedString::from(label))
            .when_some(tooltip, |trigger, tip| {
                trigger.tooltip(move |window, cx| {
                    Tooltip::new(SharedString::from(tip.clone())).build(window, cx)
                })
            })
            .child({
                let weak = cx.weak_entity();
                canvas(
                    move |bounds, _window, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if this.trigger_bounds != Some(bounds) {
                                this.trigger_bounds = Some(bounds);
                                cx.notify();
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            })
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                if has_click {
                    this.center
                        .update(cx, |center, cx| center.activate(primary_id, false, cx));
                } else if this.is_open() {
                    this.close(cx);
                } else {
                    this.presence.open();
                    this.fallback_anchor = ev.position();
                    window.focus(&this.focus);
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                if this.is_open() && ev.keystroke.key == "escape" {
                    this.close(cx);
                    cx.stop_propagation();
                }
            }));

        if !self.presence.is_mounted() {
            return div().flex_shrink_0().child(trigger).into_any_element();
        }
        let anchor = self
            .trigger_bounds
            .map(|bounds| bounds.bottom_left())
            .unwrap_or(self.fallback_anchor);
        let view = cx.entity();
        let content = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(palette.border)
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(palette.fg)
                    .child("Activity"),
            )
            .child(self.render_popover_rows(entries, palette, cx))
            .into_any_element();
        div()
            .relative()
            .flex_shrink_0()
            .child(trigger)
            .child(popover_with_exit(
                anchor,
                px(320.0),
                palette,
                self.presence.is_closing(),
                move |_window: &mut Window, cx: &mut App| {
                    view.update(cx, |item, cx| item.close(cx));
                },
                content,
            ))
            .into_any_element()
    }
}

/// Build the activity status-bar contribution for application composition.
pub fn registration(
    center: &Entity<ActivityCenter>,
    theme: &Entity<ThemeStore>,
    cx: &mut App,
) -> StatusItemRegistration {
    let item = cx.new(|cx| ActivityStatusItem::new(center.clone(), theme.clone(), cx));
    let handle = item.clone();
    StatusItemRegistration {
        id: item.read(cx).id(),
        default_side: item.read(cx).default_side(),
        order: item.read(cx).order(),
        group: item.read(cx).group(),
        build: Arc::new(move |_window, _cx| Arc::new(handle.clone()) as AnyStatusItemHandle),
    }
}
