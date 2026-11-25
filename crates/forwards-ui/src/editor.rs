// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! The forward editor dialog body. Hosted by the shell's modal layer; emits
//! typed [`ForwardEditorEvent`]s and never talks to services.

use gpui::{
    div, prelude::FluentBuilder, px, AnyElement, App, AppContext as _, ClickEvent, Context, Entity,
    EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use labonair_forwards::{
    DraftField, EditorDefaults, FieldError, ForwardDraft, ForwardEditorEvent, QuickSpec,
    SavedForward,
};
use labonair_ssh::{ForwardKind, ForwardLifetime};
use labonair_theme::store::ThemeStore;
use labonair_ui_kit::{
    button, checkbox, dialog_surface, segmented_control, text_field, text_field_surface,
    text_input, ButtonSize, ButtonVariant, InputEvent, InputState, Palette, TextFieldState,
};

const BIND_PRESETS: [&str; 3] = ["127.0.0.1", "localhost", "0.0.0.0"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Start,
    SaveAndStart,
    SaveOnly,
}

pub struct ForwardEditor {
    theme: Entity<ThemeStore>,
    defaults: EditorDefaults,
    hosts: Vec<(String, String)>,
    host_id: Option<String>,
    /// The picker stays visible when the editor opened without a host.
    pick_host: bool,
    draft: ForwardDraft,
    confirmed_public: bool,
    /// Saved forwards of all hosts, for the port-conflict hint.
    saved: Vec<labonair_forwards::HostForwards>,
    /// Inline errors appear after the first failed submit attempt.
    show_errors: bool,
    name: Entity<InputState>,
    listen_port: Entity<InputState>,
    target_host: Entity<InputState>,
    target_port: Entity<InputState>,
    focus: FocusHandle,
}

impl EventEmitter<ForwardEditorEvent> for ForwardEditor {}

impl ForwardEditor {
    /// `hosts` are `(id, name)` pairs for the picker (shown only when
    /// `host_id` is `None`); `edit` (a saved forward) or `quick` prefills the draft.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        theme: Entity<ThemeStore>,
        hosts: Vec<(String, String)>,
        host_id: Option<String>,
        quick: Option<&QuickSpec>,
        edit: Option<&SavedForward>,
        defaults: EditorDefaults,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let draft = match (edit, quick) {
            (Some(saved), _) => ForwardDraft::from_saved(saved),
            (None, Some(q)) => ForwardDraft::from_quick(q, &defaults),
            (None, None) => ForwardDraft::new(&defaults),
        };
        let mut input = |value: &str, placeholder: &'static str| {
            let value = value.to_string();
            let state = cx.new(|cx| text_field(window, cx).placeholder(placeholder));
            state.update(cx, |s, cx| s.set_value(value, window, cx));
            state
        };
        let name = input(&draft.name, "Optional label");
        let listen_port = input(&draft.listen_port, "8080 (0 = any free port)");
        let target_host = input(&draft.target_host, "db.internal");
        let target_port = input(&draft.target_port, "5432");
        for (state, field) in [
            (&name, None),
            (&listen_port, Some(DraftField::ListenPort)),
            (&target_host, Some(DraftField::TargetHost)),
            (&target_port, Some(DraftField::TargetPort)),
        ] {
            cx.subscribe(
                state,
                move |this, input, event: &InputEvent, cx| match event {
                    InputEvent::Change => {
                        let v = input.read(cx).value().to_string();
                        match field {
                            None => this.draft.name = v,
                            Some(DraftField::ListenPort) => this.draft.listen_port = v,
                            Some(DraftField::TargetHost) => this.draft.target_host = v,
                            Some(_) => this.draft.target_port = v,
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.submit(Action::Start, cx),
                    _ => {}
                },
            )
            .detach();
        }
        let focus = listen_port.read(cx).focus_handle(cx);
        cx.observe(&theme, |_, _, cx| cx.notify()).detach();
        Self {
            theme,
            defaults,
            hosts,
            pick_host: host_id.is_none(),
            host_id,
            draft,
            confirmed_public: false,
            saved: Vec::new(),
            show_errors: false,
            name,
            listen_port,
            target_host,
            target_port,
            focus,
        }
    }

    /// Provide every host's saved forwards so the editor can warn about
    /// a listen address another host already uses (not blocking).
    pub fn with_saved_forwards(mut self, saved: Vec<labonair_forwards::HostForwards>) -> Self {
        self.saved = saved;
        self
    }

    fn conflicts(&self) -> Vec<String> {
        let Some(host_id) = self.host_id.as_deref() else {
            return Vec::new();
        };
        match self.draft.to_saved(host_id) {
            Ok(saved) => labonair_forwards::listen_conflicts(&self.saved, host_id, &saved),
            Err(_) => Vec::new(),
        }
    }

    pub fn draft(&self) -> &ForwardDraft {
        &self.draft
    }

    fn needs_public_confirm(&self) -> bool {
        self.defaults.confirm_public_bind && self.draft.public_bind_warning().is_some()
    }

    fn submit(&mut self, action: Action, cx: &mut Context<Self>) {
        let (Some(host_id), true) = (
            self.host_id.clone(),
            self.draft.validate().is_empty()
                && (!self.needs_public_confirm() || self.confirmed_public),
        ) else {
            self.show_errors = true;
            cx.notify();
            return;
        };
        let (Ok(spec), Ok(saved)) = (self.draft.to_spec(&host_id), self.draft.to_saved(&host_id))
        else {
            return;
        };
        let event = match action {
            Action::Start => ForwardEditorEvent::Submit {
                spec,
                lifetime: ForwardLifetime::Pinned,
                save_to_host: None,
                start: true,
            },
            Action::SaveAndStart | Action::SaveOnly => ForwardEditorEvent::Submit {
                spec,
                lifetime: ForwardLifetime::WithConnection,
                save_to_host: Some(saved),
                start: action == Action::SaveAndStart,
            },
        };
        cx.emit(event);
    }

    fn set_kind(&mut self, key: &str, cx: &mut Context<Self>) {
        self.draft.kind = match key {
            "remote" => ForwardKind::Remote,
            "dynamic" => ForwardKind::Dynamic,
            _ => ForwardKind::Local,
        };
        cx.notify();
    }

    fn error_for(&self, errors: &[FieldError], field: DraftField) -> Option<&'static str> {
        errors.iter().find(|e| e.field == field).map(|e| e.message)
    }

    fn field(
        &self,
        id: &'static str,
        label: &'static str,
        input: &Entity<InputState>,
        error: Option<&'static str>,
        c: Palette,
    ) -> AnyElement {
        let state = if error.is_some() && self.show_errors {
            TextFieldState::Invalid
        } else {
            TextFieldState::Normal
        };
        div()
            .flex()
            .flex_col()
            .gap(c.space(4.0))
            .flex_1()
            .child(div().text_size(px(12.0)).text_color(c.muted).child(label))
            .child(text_field_surface(id, c, state, text_input(input, c)))
            .when_some(error.filter(|_| self.show_errors), |d, m| {
                d.child(div().text_size(px(12.0)).text_color(c.error).child(m))
            })
            .into_any_element()
    }
}

impl Focusable for ForwardEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ForwardEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = Palette::from_theme(self.theme.read(cx));
        let this = cx.entity();
        let errors = self.draft.validate();
        let kind = self.draft.kind;
        let dynamic = kind == ForwardKind::Dynamic;
        let (key, blurb) = match kind {
            ForwardKind::Local => (
                "local",
                "Open a local port that reaches a target through the host.",
            ),
            ForwardKind::Remote => (
                "remote",
                "Open a port on the host that reaches a target on this machine.",
            ),
            ForwardKind::Dynamic => (
                "dynamic",
                "Run a local SOCKS5 proxy that tunnels through the host.",
            ),
        };

        let mut binds: Vec<String> = BIND_PRESETS.iter().map(|s| s.to_string()).collect();
        if !binds.contains(&self.draft.listen_host) {
            binds.push(self.draft.listen_host.clone());
        }

        let kinds = segmented_control("fwd-kind", c, key)
            .segment("local", "Local")
            .segment("remote", "Remote")
            .segment("dynamic", "Dynamic")
            .on_select({
                let this = this.clone();
                move |k: &SharedString, _w, cx| this.update(cx, |t, cx| t.set_kind(k, cx))
            });
        let bind_select = segmented_control("fwd-bind", c, self.draft.listen_host.clone())
            .segments(binds.iter().map(|b| (b.clone(), b.clone())))
            .on_select({
                let this = this.clone();
                move |k: &SharedString, _w, cx| {
                    this.update(cx, |t, cx| {
                        t.draft.listen_host = k.to_string();
                        t.confirmed_public = false;
                        cx.notify();
                    })
                }
            });

        let host_picker = self.pick_host.then(|| {
            let mut list = div()
                .id("fwd-hosts")
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(c.space(6.0))
                .max_h(px(96.0))
                .overflow_y_scroll();
            for (id, name) in &self.hosts {
                let id = id.clone();
                list = list.child(
                    button(
                        SharedString::from(format!("fwd-host-{id}")),
                        c,
                        if self.host_id.as_deref() == Some(id.as_str()) {
                            ButtonVariant::Filled
                        } else {
                            ButtonVariant::Subtle
                        },
                        ButtonSize::Xs,
                    )
                    .child(name.clone())
                    .on_click(cx.listener(move |t, _: &ClickEvent, _w, cx| {
                        t.host_id = Some(id.clone());
                        cx.notify();
                    })),
                );
            }
            div()
                .flex()
                .flex_col()
                .gap(c.space(4.0))
                .child(div().text_size(px(12.0)).text_color(c.muted).child("Host"))
                .child(list)
                .when(self.show_errors, |d| {
                    d.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(c.error)
                            .child("Choose a host"),
                    )
                })
        });
        let warning = self.draft.public_bind_warning();
        let confirm = self.needs_public_confirm();

        dialog_surface("forward-editor", c)
            .w(px(520.0))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, ev: &KeyDownEvent, _w, cx| {
                if ev.keystroke.key == "escape" {
                    cx.emit(ForwardEditorEvent::Cancel);
                    cx.stop_propagation();
                }
            }))
            .child(div().text_size(px(15.0)).child("New port forward"))
            .child(kinds)
            .child(div().text_size(px(12.0)).text_color(c.muted).child(blurb))
            .children(host_picker)
            .child(self.field("fwd-name", "Name", &self.name.clone(), None, c))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(c.space(4.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(c.muted)
                            .child("Bind address"),
                    )
                    .child(bind_select),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(c.space(8.0))
                    .child(self.field(
                        "fwd-listen-port",
                        "Listen port",
                        &self.listen_port.clone(),
                        self.error_for(&errors, DraftField::ListenPort),
                        c,
                    ))
                    .when(!dynamic, |d| {
                        d.child(self.field(
                            "fwd-target-host",
                            "Target host",
                            &self.target_host.clone(),
                            self.error_for(&errors, DraftField::TargetHost),
                            c,
                        ))
                        .child(self.field(
                            "fwd-target-port",
                            "Target port",
                            &self.target_port.clone(),
                            self.error_for(&errors, DraftField::TargetPort),
                            c,
                        ))
                    }),
            )
            .when_some(
                self.error_for(&errors, DraftField::ListenHost)
                    .filter(|_| self.show_errors),
                |d, m| d.child(div().text_size(px(12.0)).text_color(c.error).child(m)),
            )
            .children(self.conflicts().into_iter().map(|m| {
                div()
                    .text_size(px(12.0))
                    .text_color(c.muted)
                    .child(SharedString::from(m))
            }))
            .when_some(warning, |d, w| {
                d.child(div().text_size(px(12.0)).text_color(c.warning).child(w))
            })
            .when(confirm, |d| {
                d.child(
                    checkbox("fwd-confirm-public", c, self.confirmed_public)
                        .label("I understand this forward is reachable from the network")
                        .on_click({
                            let this = this.clone();
                            move |v: &bool, _w, cx| {
                                let v = *v;
                                this.update(cx, |t, cx| {
                                    t.confirmed_public = v;
                                    cx.notify();
                                })
                            }
                        }),
                )
                .when(self.show_errors && !self.confirmed_public, |d| {
                    d.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(c.error)
                            .child("Confirm the public bind to continue"),
                    )
                })
            })
            .child(
                div()
                    .font_family("monospace")
                    .text_size(px(13.0))
                    .text_color(c.fg)
                    .child(self.draft.diagram()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(c.space(8.0))
                    .child(
                        button("fwd-cancel", c, ButtonVariant::Subtle, ButtonSize::Default)
                            .child("Cancel")
                            .on_click(cx.listener(|_, _: &ClickEvent, _w, cx| {
                                cx.emit(ForwardEditorEvent::Cancel)
                            })),
                    )
                    .child(
                        button("fwd-save", c, ButtonVariant::Outlined, ButtonSize::Default)
                            .child("Save only")
                            .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                                t.submit(Action::SaveOnly, cx)
                            })),
                    )
                    .child(
                        button(
                            "fwd-save-start",
                            c,
                            ButtonVariant::Outlined,
                            ButtonSize::Default,
                        )
                        .child("Save to host and start")
                        .on_click(cx.listener(
                            |t, _: &ClickEvent, _w, cx| t.submit(Action::SaveAndStart, cx),
                        )),
                    )
                    .child(
                        button("fwd-start", c, ButtonVariant::Action, ButtonSize::Default)
                            .child("Start")
                            .on_click(
                                cx.listener(|t, _: &ClickEvent, _w, cx| {
                                    t.submit(Action::Start, cx)
                                }),
                            ),
                    ),
            )
    }
}
