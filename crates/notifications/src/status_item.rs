// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Statusbar presentation for the notification registry.
//!
//! This belongs to the notifications capability rather than the application
//! shell. The shell only registers the item in the global statusbar registry.

use std::{collections::HashSet, sync::Arc, time::Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    canvas, div, px, AnyElement, App, AppContext, Bounds, ClickEvent, Context, Entity, FocusHandle,
    Focusable, InteractiveElement, IntoElement, ParentElement, Pixels, Point, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window,
};
use labonair_panel::{AnyStatusItemHandle, StatusItem, StatusItemRegistration, StatusSide};
use labonair_theme::store::ThemeStore;
use labonair_ui_kit::{
    empty_state, icon_button_builder, list_header, list_separator, popover_with_exit,
    search_clear_button, search_field, search_input, text_field, ButtonSize, ButtonVariant,
    IconName, InputEvent, InputState, ListItem, Palette, Presence,
};

use crate::{NotificationCenter, NotificationSnapshot, Severity};

/// The notifications status item and its statusbar dropdown.
pub struct NotificationsStatusItem {
    center: Entity<NotificationCenter>,
    theme: Entity<ThemeStore>,
    /// Open / closing / closed state of the dropdown; it stays mounted for the
    /// exit fade.
    presence: Presence,
    /// The bell trigger's window-space bounds from the last paint. The
    /// dropdown drops from `bounds.bottom_left()` so it stays anchored to the
    /// trigger, not to wherever inside it the click landed.
    trigger_bounds: Option<Bounds<Pixels>>,
    /// Click position of the press that opened the dropdown — used only until
    /// the first paint records `trigger_bounds`.
    fallback_anchor: Point<Pixels>,
    expanded: HashSet<u64>,
    search_open: bool,
    search: String,
    search_focused: bool,
    search_input: Option<Entity<InputState>>,
    _search_subscription: Option<Subscription>,
    focus: FocusHandle,
}

impl NotificationsStatusItem {
    pub fn new(
        center: Entity<NotificationCenter>,
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
            expanded: HashSet::new(),
            search_open: false,
            search: String::new(),
            search_focused: false,
            search_input: None,
            _search_subscription: None,
            focus: cx.focus_handle(),
        }
    }
}

fn notification_row(
    snapshot: NotificationSnapshot,
    expanded: bool,
    center: Entity<NotificationCenter>,
    view: Entity<NotificationsStatusItem>,
    palette: Palette,
    border: gpui::Hsla,
) -> AnyElement {
    let id = snapshot.id;
    let (icon, icon_color) = match snapshot.severity {
        Severity::Message | Severity::Info => (IconName::Info, palette.info),
        Severity::Success => (IconName::CircleCheck, palette.success),
        Severity::Warning => (IconName::TriangleAlert, palette.warning),
        Severity::Error => (IconName::CircleX, palette.error),
    };
    let leading = div()
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .child(icon.svg(icon_color).size(px(15.0)));
    let mut message = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .child(
            div()
                .text_size(px(12.0))
                .text_color(palette.fg)
                .when(!snapshot.read, |d| {
                    d.font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .child(snapshot.title),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(palette.muted)
                .child(snapshot.body),
        );
    if expanded {
        if let Some(details) = snapshot.details.clone() {
            message = message.child(
                div()
                    .pt_1()
                    .text_size(px(11.0))
                    .text_color(palette.muted)
                    .child(details),
            );
        }
    }
    if !snapshot.actions.is_empty() {
        message = message.child(div().flex().flex_wrap().gap_2().pt_1().children(
            snapshot.actions.iter().map(|action| {
                let action_id = action.id.clone();
                let action_center = center.clone();
                labonair_ui_kit::button(
                    SharedString::from(format!("bar-notification-{id}-{}", action.id)),
                    palette,
                    ButtonVariant::OutlinedGhost,
                    ButtonSize::Xs,
                )
                .child(SharedString::from(action.label.clone()))
                .on_click(move |_: &ClickEvent, _window, cx| {
                    action_center.update(cx, |center, cx| {
                        center.activate_action(id, action_id.clone(), cx)
                    });
                    cx.stop_propagation();
                })
            }),
        ));
    }

    ListItem::new(
        SharedString::from(format!("bar-notification-{id}")),
        palette,
    )
    .leading(leading)
    .on_click(move |_: &ClickEvent, _window, cx| {
        view.update(cx, |item, cx| {
            if !item.expanded.insert(id) {
                item.expanded.remove(&id);
            }
            item.center
                .update(cx, |center, cx| center.mark_read(id, cx));
            cx.notify();
        });
    })
    .extra(move |row| row.border_b_1().border_color(border))
    .child(message)
    .into_any_element()
}

impl NotificationsStatusItem {
    /// Whether the dropdown is open (not merely fading out).
    fn is_open(&self) -> bool {
        self.presence.is_mounted() && !self.presence.is_closing()
    }

    /// Start the exit fade; idempotent while closing.
    fn close(&mut self, cx: &mut Context<Self>) {
        let motion = Palette::from_theme(self.theme.read(cx)).motion;
        self.presence.request_close(Instant::now(), &motion);
        self.reset_search();
        cx.notify();
    }

    fn reset_search(&mut self) {
        self.search_open = false;
        self.search.clear();
        self.search_focused = false;
        self.search_input = None;
        self._search_subscription = None;
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open {
            self.reset_search();
        } else {
            self.search_open = true;
            let input = cx.new(|cx| text_field(window, cx).placeholder("Search notifications"));
            let subscription = cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
                match event {
                    InputEvent::Change => this.search = input.read(cx).value().to_string(),
                    InputEvent::Focus => this.search_focused = true,
                    InputEvent::Blur => this.search_focused = false,
                    _ => {}
                }
                cx.notify();
            });
            input.update(cx, |input, cx| input.focus(window, cx));
            self.search_input = Some(input);
            self._search_subscription = Some(subscription);
        }
        cx.notify();
    }
}

impl Focusable for NotificationsStatusItem {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for NotificationsStatusItem {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_status(window, cx)
    }
}

impl StatusItem for NotificationsStatusItem {
    fn id(&self) -> &'static str {
        "notifications"
    }

    fn default_side(&self) -> StatusSide {
        StatusSide::Right
    }

    fn order(&self) -> i32 {
        100
    }

    fn group(&self) -> u32 {
        2
    }

    fn render_status(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let count = self.center.read(cx).unread_count();
        let motion = Palette::from_theme(self.theme.read(cx)).motion;
        self.presence.tick(Instant::now(), &motion);
        let palette = Palette::from_theme(self.theme.read(cx));
        let accent = palette.primary;
        let border = palette.border;
        let bell = icon_button_builder("bar-notifications", palette, IconName::Bell)
            .size(ButtonSize::IconXxs)
            .selected(self.is_open())
            .render()
            .track_focus(&self.focus)
            .key_context("StatusPopover")
            .relative()
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
            .when(count > 0, |button| {
                button.child(
                    div()
                        .absolute()
                        .top(px(-2.0))
                        .right(px(-2.0))
                        .min_w(px(13.0))
                        .h(px(13.0))
                        .px(px(2.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(accent)
                        .text_color(palette.primary_fg)
                        .text_size(px(8.0))
                        .child(SharedString::from(count.to_string())),
                )
            })
            .on_click(cx.listener(|this, ev: &ClickEvent, window, cx| {
                if this.is_open() {
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
            return bell.into_any_element();
        }
        let anchor = self
            .trigger_bounds
            .map(|b| b.bottom_left())
            .unwrap_or(self.fallback_anchor);

        let snapshots = self.center.read(cx).snapshots();
        let query = self.search.to_lowercase();
        let mut problems = Vec::new();
        let mut activity = Vec::new();
        for snapshot in snapshots {
            let matches_query = query.is_empty()
                || format!(
                    "{} {} {} {}",
                    snapshot.title,
                    snapshot.body,
                    snapshot
                        .details
                        .as_ref()
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    snapshot
                        .source
                        .as_ref()
                        .map(|value| value.to_string())
                        .unwrap_or_default()
                )
                .to_lowercase()
                .contains(&query);
            if !matches_query {
                continue;
            }
            if matches!(snapshot.severity, Severity::Warning | Severity::Error) {
                problems.push(snapshot);
            } else {
                activity.push(snapshot);
            }
        }
        let view = cx.entity();
        let dismiss = {
            let view = view.clone();
            move |_window: &mut Window, cx: &mut App| {
                view.update(cx, |item, cx| item.close(cx));
            }
        };
        let mut content = div().flex().flex_col().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .py(palette.space(10.0))
                .border_b_1()
                .border_color(border)
                .child(
                    div().flex().items_center().gap_2().child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(palette.fg)
                            .child("Notifications"),
                    ),
                )
                .child(
                    div().flex().items_center().gap_1().children([
                        icon_button_builder("bar-notif-search", palette, IconName::Search)
                            .size(ButtonSize::IconXs)
                            .selected(self.search_open)
                            .tooltip("Search notifications")
                            .render()
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.toggle_search(window, cx);
                            }))
                            .into_any_element(),
                        icon_button_builder("bar-notif-clear", palette, IconName::Trash)
                            .size(ButtonSize::IconXs)
                            .disabled(self.center.read(cx).is_empty())
                            .tooltip("Clear all notifications")
                            .render()
                            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                                this.center.update(cx, |center, cx| center.clear_all(cx));
                                this.close(cx);
                            }))
                            .into_any_element(),
                    ]),
                ),
        );

        if self.search_open {
            if let Some(input) = self.search_input.clone() {
                let clear = if self.search.is_empty() {
                    None
                } else {
                    let clear_input = input.clone();
                    Some(
                        search_clear_button(
                            "bar-notif-search-clear",
                            palette,
                            move |_: &ClickEvent, window, cx| {
                                clear_input.update(cx, |input, cx| input.set_value("", window, cx));
                                cx.stop_propagation();
                            },
                        )
                        .into_any_element(),
                    )
                };
                content = content.child(div().px_3().py_2().child(search_field(
                    "bar-notif-search-field",
                    palette,
                    self.search_focused,
                    search_input(&input, palette),
                    clear,
                )));
            }
        }

        let empty_message = if self.center.read(cx).is_empty() {
            "You're all caught up"
        } else {
            "No notifications match this search"
        };
        let rows = if problems.is_empty() && activity.is_empty() {
            empty_state(empty_message, palette)
                .h(px(112.0))
                .into_any_element()
        } else {
            let has_problems = !problems.is_empty();
            let mut groups = div().flex().flex_col().gap_1();
            if !problems.is_empty() {
                let problem_count = problems.len();
                groups = groups.child(
                    div()
                        .child(list_header(
                            SharedString::from(format!("Problems · {problem_count}")),
                            palette.muted,
                        ))
                        .children(problems.into_iter().map(|snapshot| {
                            let expanded = self.expanded.contains(&snapshot.id);
                            notification_row(
                                snapshot,
                                expanded,
                                self.center.clone(),
                                view.clone(),
                                palette,
                                border,
                            )
                        })),
                );
            }
            if !activity.is_empty() {
                if has_problems {
                    groups = groups.child(list_separator(border));
                }
                let activity_count = activity.len();
                groups = groups.child(
                    div()
                        .child(list_header(
                            SharedString::from(format!("Activity · {activity_count}")),
                            palette.muted,
                        ))
                        .children(activity.into_iter().map(|snapshot| {
                            let expanded = self.expanded.contains(&snapshot.id);
                            notification_row(
                                snapshot,
                                expanded,
                                self.center.clone(),
                                view.clone(),
                                palette,
                                border,
                            )
                        })),
                );
            }
            div()
                .id("bar-notifications-list")
                .max_h(px(360.0))
                .overflow_y_scroll()
                .px_2()
                .pb_2()
                .child(groups)
                .into_any_element()
        };
        content = content.child(rows);

        div()
            .relative()
            .flex_shrink_0()
            .child(bell)
            .child(popover_with_exit(
                anchor,
                px(360.0),
                palette,
                self.presence.is_closing(),
                dismiss,
                content.into_any_element(),
            ))
            .into_any_element()
    }
}

/// Build the notification status-bar contribution for application composition.
pub fn registration(
    center: &Entity<NotificationCenter>,
    theme: &Entity<ThemeStore>,
    cx: &mut App,
) -> StatusItemRegistration {
    let item = cx.new(|cx| NotificationsStatusItem::new(center.clone(), theme.clone(), cx));
    let handle = item.clone();
    StatusItemRegistration {
        id: item.read(cx).id(),
        default_side: item.read(cx).default_side(),
        order: item.read(cx).order(),
        group: item.read(cx).group(),
        build: Arc::new(move |_window, _cx| Arc::new(handle.clone()) as AnyStatusItemHandle),
    }
}
