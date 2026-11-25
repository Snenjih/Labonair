// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Modal that answers the pending prompt of an `AwaitingInput` forward. It
//! emits typed [`ForwardPromptEvent`]s; the workspace performs the answer.

use gpui::{
    div, px, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window,
};
use labonair_ssh::{ForwardId, PendingPrompt, PromptAnswer, PromptKind, PromptRequestId};
use labonair_theme::store::ThemeStore;
use labonair_ui_kit::{
    button, dialog_surface, text_field, text_field_surface, text_input, ButtonSize, ButtonVariant,
    InputEvent, InputState, Palette, TextFieldState,
};

/// What the dialog asks the workspace to do. Answers carry the forward and
/// request ids so a stale answer can be rejected by the service.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ForwardPromptEvent {
    Answer {
        forward_id: ForwardId,
        request_id: PromptRequestId,
        answer: PromptAnswer,
    },
    Cancel,
}

pub struct ForwardPromptDialog {
    theme: Entity<ThemeStore>,
    forward_id: ForwardId,
    pending: PendingPrompt,
    /// Masked input for password / passphrase prompts.
    secret: Option<Entity<InputState>>,
    focus: FocusHandle,
}

impl EventEmitter<ForwardPromptEvent> for ForwardPromptDialog {}

/// Dialog title; mirrors the SSH loading screen's `Hop i/n: label: what`.
fn title(pending: &PendingPrompt) -> String {
    let what = match pending.kind {
        PromptKind::HostKey { .. } => "Verify host key",
        PromptKind::Password => "Password required",
        PromptKind::Passphrase => "Key passphrase required",
    };
    match &pending.hop {
        Some(h) => format!("Hop {}/{}: {}: {what}", h.index, h.total, h.label),
        None => what.to_string(),
    }
}

impl ForwardPromptDialog {
    pub fn new(
        theme: Entity<ThemeStore>,
        forward_id: ForwardId,
        pending: PendingPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let secret = (!matches!(pending.kind, PromptKind::HostKey { .. })).then(|| {
            let state = cx.new(|cx| text_field(window, cx).masked(true));
            cx.subscribe(&state, |this, _, event: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.submit(cx);
                }
            })
            .detach();
            state
        });
        let focus = match &secret {
            Some(s) => s.read(cx).focus_handle(cx),
            None => cx.focus_handle(),
        };
        cx.observe(&theme, |_, _, cx| cx.notify()).detach();
        Self {
            theme,
            forward_id,
            pending,
            secret,
            focus,
        }
    }

    pub fn forward_id(&self) -> &ForwardId {
        &self.forward_id
    }

    pub fn request_id(&self) -> &PromptRequestId {
        &self.pending.request_id
    }

    fn answer(&mut self, answer: PromptAnswer, cx: &mut Context<Self>) {
        cx.emit(ForwardPromptEvent::Answer {
            forward_id: self.forward_id.clone(),
            request_id: self.pending.request_id.clone(),
            answer,
        });
    }

    /// Enter / Authenticate: send the typed secret and wipe the input.
    fn submit(&mut self, cx: &mut Context<Self>) {
        let Some(input) = self.secret.clone() else {
            return;
        };
        let value = input.read(cx).value().to_string();
        self.answer(PromptAnswer::Secret(Some(value)), cx);
    }

    /// Esc / Reject: decline without sending anything.
    fn reject(&mut self, cx: &mut Context<Self>) {
        let answer = match self.pending.kind {
            PromptKind::HostKey { .. } => PromptAnswer::Trust(false),
            _ => PromptAnswer::Secret(None),
        };
        self.answer(answer, cx);
    }
}

impl Focusable for ForwardPromptDialog {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ForwardPromptDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = Palette::from_theme(self.theme.read(cx));
        let body = match (&self.pending.kind, &self.secret) {
            (
                PromptKind::HostKey {
                    host,
                    fingerprint,
                    is_mismatch,
                },
                _,
            ) => {
                let mut d = div()
                    .flex()
                    .flex_col()
                    .gap(c.space(6.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(c.muted)
                            .child(format!("The authenticity of {host} cannot be established.")),
                    )
                    .child(
                        div()
                            .font_family("monospace")
                            .text_size(px(12.0))
                            .child(fingerprint.clone()),
                    );
                if *is_mismatch {
                    d = d.child(div().text_size(px(12.0)).text_color(c.error).child(
                        "WARNING: the host key has CHANGED. This could be a man-in-the-middle attack.",
                    ));
                }
                d
            }
            (_, Some(input)) => div().child(text_field_surface(
                "fwd-prompt-secret",
                c,
                TextFieldState::Normal,
                text_input(input, c),
            )),
            _ => div(),
        };
        let actions = match self.pending.kind {
            PromptKind::HostKey { .. } => div()
                .flex()
                .flex_row()
                .justify_end()
                .gap(c.space(8.0))
                .child(
                    button(
                        "fwd-prompt-reject",
                        c,
                        ButtonVariant::Subtle,
                        ButtonSize::Default,
                    )
                    .child("Reject")
                    .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.reject(cx))),
                )
                .child(
                    button(
                        "fwd-prompt-trust",
                        c,
                        ButtonVariant::Action,
                        ButtonSize::Default,
                    )
                    .child("Trust")
                    .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| {
                        t.answer(PromptAnswer::Trust(true), cx)
                    })),
                ),
            _ => div()
                .flex()
                .flex_row()
                .justify_end()
                .gap(c.space(8.0))
                .child(
                    button(
                        "fwd-prompt-cancel",
                        c,
                        ButtonVariant::Subtle,
                        ButtonSize::Default,
                    )
                    .child("Cancel")
                    .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.reject(cx))),
                )
                .child(
                    button(
                        "fwd-prompt-ok",
                        c,
                        ButtonVariant::Action,
                        ButtonSize::Default,
                    )
                    .child("Authenticate")
                    .on_click(cx.listener(|t, _: &ClickEvent, _w, cx| t.submit(cx))),
                ),
        };
        let _ = window;
        dialog_surface("forward-prompt", c)
            .w(px(440.0))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|t, ev: &KeyDownEvent, _w, cx| {
                if ev.keystroke.key == "escape" {
                    t.reject(cx);
                    cx.stop_propagation();
                }
            }))
            .child(div().text_size(px(15.0)).child(title(&self.pending)))
            .child(body)
            .child(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use labonair_ssh::HopInfo;

    #[test]
    fn title_includes_hop_label() {
        let mut p = PendingPrompt {
            request_id: PromptRequestId::new("r"),
            kind: PromptKind::Password,
            hop: None,
        };
        assert_eq!(title(&p), "Password required");
        p.hop = Some(HopInfo {
            index: 2,
            total: 3,
            label: "bastion".into(),
        });
        assert_eq!(title(&p), "Hop 2/3: bastion: Password required");
    }
}
