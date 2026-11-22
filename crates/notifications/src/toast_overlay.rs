// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! App-wide transient presentation for notifications that opt into a toast.

use gpui::{
    div, px, App, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled,
};
use labonair_theme::store::ThemeStore;
use labonair_ui_kit::{icon_button_builder, ButtonSize, ButtonVariant, IconName, Palette};

use crate::{NotificationCenter, NotificationSnapshot, Severity};

pub struct NotificationToastOverlay {
    center: Entity<NotificationCenter>,
    theme: Entity<ThemeStore>,
}

impl NotificationToastOverlay {
    fn new(
        center: Entity<NotificationCenter>,
        theme: Entity<ThemeStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&center, |_, _, cx| cx.notify()).detach();
        cx.observe(&theme, |_, _, cx| cx.notify()).detach();
        Self { center, theme }
    }
}

fn severity_style(severity: Severity, palette: Palette) -> (IconName, gpui::Hsla) {
    match severity {
        Severity::Message | Severity::Info => (IconName::Info, palette.info),
        Severity::Success => (IconName::CircleCheck, palette.success),
        Severity::Warning => (IconName::TriangleAlert, palette.warning),
        Severity::Error => (IconName::CircleX, palette.error),
    }
}

fn toast_card(
    snapshot: NotificationSnapshot,
    center: Entity<NotificationCenter>,
    palette: Palette,
) -> impl IntoElement {
    let id = snapshot.id;
    let (icon, color) = severity_style(snapshot.severity, palette);
    let actions = snapshot.actions;
    let mut content = div()
        .flex()
        .items_center()
        .gap(palette.space(12.0))
        .flex_1()
        .min_w_0()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(palette.space(2.0))
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(palette.fg)
                        .child(snapshot.title),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(palette.muted)
                        .child(snapshot.body),
                ),
        );

    if !actions.is_empty() {
        content = content.children(actions.into_iter().map(|action| {
            let action_id = action.id.clone();
            let action_center = center.clone();
            labonair_ui_kit::button(
                SharedString::from(format!("notification-toast-{id}-{}", action.id)),
                palette,
                ButtonVariant::Action,
                ButtonSize::Xs,
            )
            .child(action.label)
            .on_click(move |_, _, cx| {
                action_center.update(cx, |center, cx| {
                    center.activate_action(id, action_id.clone(), cx)
                });
                cx.stop_propagation();
            })
        }));
    }

    div()
        .flex()
        .items_center()
        .gap(palette.space(10.0))
        .w_full()
        .px(palette.space(14.0))
        .py(palette.space(11.0))
        .rounded(px(palette.radius.lg))
        .border_1()
        .border_color(palette.border)
        .bg(palette.popover)
        .shadow(palette.popover_shadow())
        .child(icon.svg(color).size(px(18.0)))
        .child(content)
        .child(
            icon_button_builder(
                SharedString::from(format!("notification-toast-{id}-dismiss")),
                palette,
                IconName::X,
            )
            .size(ButtonSize::IconXs)
            .tooltip("Dismiss notification")
            .render()
            .on_click(move |_, _, cx| {
                center.update(cx, |center, cx| center.dismiss_toast(id, cx));
                cx.stop_propagation();
            }),
        )
}

impl Render for NotificationToastOverlay {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notifications = self.center.read(cx).toast_snapshots();
        if notifications.is_empty() {
            return div().into_any_element();
        }
        let palette = Palette::from_theme(self.theme.read(cx));
        div()
            .absolute()
            .top(px(18.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .px(palette.space(16.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(palette.space(8.0))
                    .w_full()
                    .max_w(px(480.0))
                    .children(
                        notifications
                            .into_iter()
                            .map(|snapshot| toast_card(snapshot, self.center.clone(), palette)),
                    ),
            )
            .into_any_element()
    }
}

pub fn registration(
    center: &Entity<NotificationCenter>,
    theme: &Entity<ThemeStore>,
    cx: &mut App,
) -> Entity<NotificationToastOverlay> {
    cx.new(|cx| NotificationToastOverlay::new(center.clone(), theme.clone(), cx))
}
