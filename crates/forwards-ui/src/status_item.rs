// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! The `forwards` statusbar item: a show-on-demand badge (count + health dot)
//! with a popover listing forwards grouped by host.

use std::sync::Arc;
use std::time::Instant;

use gpui::{
    canvas, div, px, AnyElement, App, AppContext, Bounds, ClickEvent, Context, Entity, FocusHandle,
    Focusable, InteractiveElement, IntoElement, ParentElement, Pixels, Point, Render, SharedString,
    StatefulInteractiveElement, Styled, Window,
};
use labonair_forwards::{kind_glyph, route_label, ForwardsUiEvent};
use labonair_panel::{AnyStatusItemHandle, StatusItem, StatusItemRegistration, StatusSide};
use labonair_ssh::ForwardState;
use labonair_theme::store::ThemeStore;
use labonair_ui_kit::{
    button, icon_button_builder, indicator, popover_with_exit, ButtonSize, ButtonVariant, IconName,
    IndicatorSize, ListItem, Palette, Presence,
};

use crate::rows::browser_url;
use crate::{aggregate_health, group_by_host, state_text, ForwardRow, ForwardsView, Health};

pub struct ForwardsStatusItem {
    view: Entity<ForwardsView>,
    theme: Entity<ThemeStore>,
    presence: Presence,
    trigger_bounds: Option<Bounds<Pixels>>,
    fallback_anchor: Point<Pixels>,
    focus: FocusHandle,
}

fn health_color(health: Health, palette: &Palette) -> gpui::Hsla {
    match health {
        Health::Active => palette.success,
        Health::Busy => palette.warning,
        Health::NeedsInput => palette.info,
        Health::Failed => palette.error,
    }
}

impl ForwardsStatusItem {
    pub fn new(
        view: Entity<ForwardsView>,
        theme: Entity<ThemeStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&view, |this, view, cx| {
            if view.update(cx, |v, _| v.take_open_request()) && !this.is_open() {
                this.presence.open();
            }
            cx.notify();
        })
        .detach();
        cx.observe(&theme, |_, _, cx| cx.notify()).detach();
        Self {
            view,
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
        self.view.update(cx, |view, cx| view.clear_selection(cx));
        cx.notify();
    }

    /// Emit a request from a popover control; `close` also dismisses the popover.
    fn send(&mut self, event: ForwardsUiEvent, close: bool, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| view.request(event, cx));
        if close {
            self.close(cx);
        }
    }

    fn row_button(
        &self,
        key: String,
        icon: IconName,
        tip: &'static str,
        palette: Palette,
        event: ForwardsUiEvent,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        icon_button_builder(SharedString::from(key), palette, icon)
            .size(ButtonSize::IconXxs)
            .tooltip(tip)
            .render()
            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                cx.stop_propagation();
                this.send(event.clone(), false, cx);
            }))
            .into_any_element()
    }

    fn render_row(
        &self,
        index: usize,
        row: &ForwardRow,
        selected: bool,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let info = &row.info;
        let id = info.spec.id.clone();
        let health = crate::aggregate_health(std::slice::from_ref(info));
        let mut title = route_label(&info.spec);
        if let Some(name) = info.spec.name.as_deref().filter(|n| !n.is_empty()) {
            title = format!("{name}  {title}");
        }
        let mut detail = state_text(&info.state);
        if info.connections > 0 {
            detail.push_str(&format!(
                " · {} connection{}",
                info.connections,
                if info.connections == 1 { "" } else { "s" }
            ));
        }
        let mut actions = div().flex().items_center().gap_0p5();
        if matches!(info.state, ForwardState::AwaitingInput(_)) {
            actions = actions.child(
                button(
                    SharedString::from(format!("fwd-auth-{index}")),
                    palette,
                    ButtonVariant::Outlined,
                    ButtonSize::Xs,
                )
                .child("Authenticate")
                .on_click(cx.listener({
                    let id = id.clone();
                    move |this, _: &ClickEvent, _w, cx| {
                        cx.stop_propagation();
                        this.send(ForwardsUiEvent::Prompt(id.clone()), false, cx);
                    }
                })),
            );
        }
        actions = actions.child(self.row_button(
            format!("fwd-copy-{index}"),
            IconName::Copy,
            "Copy address",
            palette,
            ForwardsUiEvent::CopyAddress(info.address_label()),
            cx,
        ));
        if let Some(url) = browser_url(info) {
            actions = actions.child(self.row_button(
                format!("fwd-open-{index}"),
                IconName::ArrowUpRight,
                "Open in browser",
                palette,
                ForwardsUiEvent::OpenInBrowser(url),
                cx,
            ));
        }
        if matches!(
            info.state,
            ForwardState::Failed { .. } | ForwardState::Stopped
        ) {
            actions = actions.child(self.row_button(
                format!("fwd-restart-{index}"),
                IconName::Refresh,
                "Restart",
                palette,
                ForwardsUiEvent::Restart(id.clone()),
                cx,
            ));
        }
        actions = actions.child(self.row_button(
            format!("fwd-close-{index}"),
            IconName::X,
            "Close forward",
            palette,
            ForwardsUiEvent::Close(id),
            cx,
        ));
        ListItem::new(SharedString::from(format!("fwd-row-{index}")), palette)
            .selected(selected)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(indicator(IndicatorSize::Sm, health_color(health, &palette)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().text_xs().text_color(palette.fg).truncate().child(
                                SharedString::from(format!(
                                    "{} {title}",
                                    kind_glyph(info.spec.kind)
                                )),
                            ))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(palette.muted)
                                    .truncate()
                                    .child(SharedString::from(detail)),
                            ),
                    )
                    .child(actions),
            )
            .into_any_element()
    }

    fn render_popover(&mut self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.view.read(cx).rows().to_vec();
        let selected = self.view.read(cx).selected();
        let header = div()
            .flex()
            .items_center()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(palette.fg)
                    .child("Port forwards"),
            )
            .child(
                button("fwd-new", palette, ButtonVariant::Subtle, ButtonSize::Xs)
                    .child("New\u{2026}")
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        this.send(
                            ForwardsUiEvent::New {
                                host_id: None,
                                quick: None,
                            },
                            true,
                            cx,
                        );
                    })),
            )
            .child(
                button(
                    "fwd-stop-all",
                    palette,
                    ButtonVariant::Subtle,
                    ButtonSize::Xs,
                )
                .child("Stop all")
                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                    this.send(ForwardsUiEvent::StopAll, false, cx);
                })),
            );
        let mut list = div()
            .id("forwards-list")
            .max_h(px(360.0))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for group in group_by_host(&rows) {
            let mut title = div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .pt_2()
                .pb_1()
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(palette.fg)
                        .child(SharedString::from(group.label.clone())),
                );
            if !group.route.is_empty() {
                title = title.child(
                    div()
                        .px_1()
                        .rounded_sm()
                        .bg(palette.border)
                        .text_size(px(10.0))
                        .text_color(palette.muted)
                        .child(SharedString::from(format!(
                            "via {}",
                            group.route.join(" \u{203a} ")
                        ))),
                );
            }
            list = list.child(title);
            for index in group.rows {
                list = list.child(self.render_row(
                    index,
                    &rows[index],
                    selected == Some(index),
                    palette,
                    cx,
                ));
            }
        }
        let footer = div()
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(palette.border)
            .child(
                button("fwd-manage", palette, ButtonVariant::Subtle, ButtonSize::Xs)
                    .child("Manage in Hosts")
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        this.send(ForwardsUiEvent::Manage { host_id: None }, true, cx);
                    })),
            );
        div()
            .flex()
            .flex_col()
            .child(header)
            .child(list)
            .child(footer)
            .into_any_element()
    }
}

impl Focusable for ForwardsStatusItem {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ForwardsStatusItem {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_status(window, cx)
    }
}

impl StatusItem for ForwardsStatusItem {
    fn id(&self) -> &'static str {
        "forwards"
    }

    fn default_side(&self) -> StatusSide {
        StatusSide::Right
    }

    fn order(&self) -> i32 {
        15
    }

    fn group(&self) -> u32 {
        1
    }

    fn is_empty(&self, cx: &App) -> bool {
        self.view.read(cx).rows().is_empty()
    }

    fn render_status(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (count, health) = {
            let rows = self.view.read(cx).rows();
            let infos: Vec<_> = rows.iter().map(|r| r.info.clone()).collect();
            (rows.len(), aggregate_health(&infos))
        };
        if count == 0 {
            self.presence = Presence::new();
            return div().into_any_element();
        }
        let palette = Palette::from_theme(self.theme.read(cx));
        self.presence.tick(Instant::now(), &palette.motion);

        let trigger = div()
            .id("forwards-trigger")
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
            .child(
                icon_button_builder("bar-forwards", palette, IconName::Link)
                    .size(ButtonSize::IconXxs)
                    .indicator(IndicatorSize::Xs, health_color(health, &palette))
                    .render(),
            )
            .child(SharedString::from(count.to_string()))
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
                if !this.is_open() {
                    return;
                }
                match ev.keystroke.key.as_str() {
                    "escape" => this.close(cx),
                    "down" => this.view.update(cx, |v, cx| v.move_selection(1, cx)),
                    "up" => this.view.update(cx, |v, cx| v.move_selection(-1, cx)),
                    "enter" => this.view.update(cx, |v, cx| v.activate_selected(cx)),
                    _ => return,
                }
                cx.stop_propagation();
            }));

        if !self.presence.is_mounted() {
            return div().flex_shrink_0().child(trigger).into_any_element();
        }
        let anchor = self
            .trigger_bounds
            .map(|bounds| bounds.bottom_left())
            .unwrap_or(self.fallback_anchor);
        let me = cx.entity();
        let content = self.render_popover(palette, cx);
        div()
            .relative()
            .flex_shrink_0()
            .child(trigger)
            .child(popover_with_exit(
                anchor,
                px(380.0),
                palette,
                self.presence.is_closing(),
                move |_window: &mut Window, cx: &mut App| {
                    me.update(cx, |item, cx| item.close(cx));
                },
                content,
            ))
            .into_any_element()
    }
}

/// Build the forwards status-bar contribution for application composition.
pub fn status_item_registration(
    view: &Entity<ForwardsView>,
    theme: &Entity<ThemeStore>,
    cx: &mut App,
) -> StatusItemRegistration {
    let item = cx.new(|cx| ForwardsStatusItem::new(view.clone(), theme.clone(), cx));
    let handle = item.clone();
    StatusItemRegistration {
        id: item.read(cx).id(),
        default_side: item.read(cx).default_side(),
        order: item.read(cx).order(),
        group: item.read(cx).group(),
        build: Arc::new(move |_window, _cx| Arc::new(handle.clone()) as AnyStatusItemHandle),
    }
}
