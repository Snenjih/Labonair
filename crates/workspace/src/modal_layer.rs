// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! [`ModalLayer`] + [`ModalView`] — the workspace's single modal overlay slot
//! (T17-005).
//!
//! Labonair needs exactly **one** active, focus-trapping modal at a time (no
//! modal stack, no reopenable-picker stash, no dismiss-veto protocol).
//!
//! A [`ModalView`] is any `Render + Focusable + EventEmitter<DismissEvent>`
//! view ([`gpui::ManagedView`]). [`ModalLayer::toggle_modal`] /
//! [`ModalLayer::open_modal`] build it, move focus into it, and drop it again
//! when it emits [`DismissEvent`] (Esc / overlay click / programmatic close)
//! or — when `dismiss_on_focus_lost` is set — when focus leaves it.
//!
//! The two existing overlays (command palette, updater dialog) still paint
//! their own full-screen scrim + centered card and handle their own Esc /
//! overlay click, so they set [`ModalView::render_bare`] to
//! `true`: the layer hosts them for lifecycle + focus and renders them as-is.
//! New modals (e.g. the `Cmd+F` search overlay, T18-002) can rely on the
//! default backdrop path this module provides.

use gpui::{
    div, AnyView, App, AppContext, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    InteractiveElement, IntoElement, ManagedView, MouseButton, MouseDownEvent, ParentElement,
    Render, Styled, Subscription, Window,
};

use crate::theme::modal_scrim;

/// A view that can be hosted by the [`ModalLayer`].
pub trait ModalView: ManagedView {
    /// Called right before the layer drops this modal. Views that keep their
    /// own `open` flag (e.g. the palette) reset it here so the two states
    /// can never drift apart.
    fn on_dismiss(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// When `true`, the layer renders the view directly with **no** backdrop
    /// and **no** centering wrapper — the view already paints its own
    /// full-screen overlay (scrim, positioning, Esc, overlay-click). Defaults
    /// to `false` (the layer supplies a scrim + a centered panel).
    fn render_bare(&self) -> bool {
        false
    }
}

type DismissHook = Box<dyn Fn(&mut Window, &mut App)>;
type BareProbe = Box<dyn Fn(&App) -> bool>;

/// The modal currently on screen, type-erased at construction time.
struct Presented {
    view: AnyView,
    view_focus: FocusHandle,
    /// Wrapper focus target used by the non-bare layout.
    wrapper_focus: FocusHandle,
    /// Focus owner to hand focus back to when the modal goes away.
    return_focus: Option<FocusHandle>,
    notify_dismiss: DismissHook,
    is_bare: BareProbe,
    _subscriptions: [Subscription; 2],
}

/// The single modal-overlay slot. Held as an [`Entity`] by the shell and
/// rendered as the workspace composition's only global overlay. Notifications
/// are rendered by the statusbar dropdown instead.
#[derive(Default)]
pub struct ModalLayer {
    current: Option<Presented>,
    dismiss_on_focus_lost: bool,
}

impl ModalLayer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle a modal of type `V`: a visible modal of the same type is closed;
    /// otherwise any other modal is replaced by the freshly built one.
    pub fn toggle_modal<V, B>(&mut self, window: &mut Window, cx: &mut Context<Self>, build: B)
    where
        V: ModalView,
        B: FnOnce(&mut Window, &mut Context<V>) -> V,
    {
        if self.active_modal::<V>().is_some() {
            self.hide_modal(window, cx);
        } else {
            self.open_modal(window, cx, build);
        }
    }

    /// Show `V` unconditionally, replacing any current modal. Used by modals
    /// whose visibility follows external state rather than a user toggle
    /// (the updater dialog).
    pub fn open_modal<V, B>(&mut self, window: &mut Window, cx: &mut Context<Self>, build: B)
    where
        V: ModalView,
        B: FnOnce(&mut Window, &mut Context<V>) -> V,
    {
        // Focus must return to what was focused before the *first* modal of a
        // replacement chain, not to the modal being replaced.
        let return_focus = match self.current.as_ref() {
            Some(old) => old.return_focus.clone(),
            None => window.focused(cx),
        };
        self.hide_modal(window, cx);

        let entity = cx.new(|cx| build(window, cx));
        let view_focus = entity.read(cx).focus_handle(cx);
        let wrapper_focus = cx.focus_handle();

        let on_dismiss_event =
            cx.subscribe_in(&entity, window, |this, _, _: &DismissEvent, window, cx| {
                this.hide_modal(window, cx);
            });
        let on_focus_out = cx.on_focus_out(&wrapper_focus, window, |this, _, window, cx| {
            if this.dismiss_on_focus_lost {
                this.hide_modal(window, cx);
            }
        });

        let for_dismiss = entity.clone();
        let for_bare = entity.clone();
        self.current = Some(Presented {
            view: entity.into(),
            view_focus: view_focus.clone(),
            wrapper_focus,
            return_focus,
            notify_dismiss: Box::new(move |window, cx| {
                for_dismiss.update(cx, |modal, cx| modal.on_dismiss(window, cx));
            }),
            is_bare: Box::new(move |cx| for_bare.read(cx).render_bare()),
            _subscriptions: [on_dismiss_event, on_focus_out],
        });

        cx.defer_in(window, move |_, window, _| window.focus(&view_focus));
        cx.notify();
    }

    /// Hide the active modal (if any), restoring focus to whatever held it
    /// before the modal opened. Returns `true` when a modal was hidden.
    pub fn hide_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.dismiss_on_focus_lost = false;
        let Some(presented) = self.current.take() else {
            return false;
        };
        (presented.notify_dismiss)(window, cx);

        // Bare modals never mount the wrapper handle, so check the modal's own
        // handle too before pulling focus back.
        let held_focus = presented.wrapper_focus.contains_focused(window, cx)
            || presented.view_focus.contains_focused(window, cx);
        if let (true, Some(target)) = (held_focus, presented.return_focus.as_ref()) {
            target.focus(window);
        }
        cx.notify();
        true
    }

    /// Whether the layer should drop the modal when focus leaves it.
    pub fn set_dismiss_on_focus_lost(&mut self, value: bool) {
        self.dismiss_on_focus_lost = value;
    }

    /// The active modal, if it is of type `V`.
    pub fn active_modal<V: 'static>(&self) -> Option<Entity<V>> {
        self.current.as_ref()?.view.clone().downcast::<V>().ok()
    }

    pub fn has_active_modal(&self) -> bool {
        self.current.is_some()
    }
}

impl EventEmitter<DismissEvent> for ModalLayer {}

impl Render for ModalLayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(presented) = &self.current else {
            return div().into_any_element();
        };

        // Bare modals paint their own scrim / positioning / key handling.
        if (presented.is_bare)(cx) {
            return presented.view.clone().into_any_element();
        }

        let panel = div()
            .occlude()
            .track_focus(&presented.wrapper_focus)
            .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
            })
            .child(presented.view.clone());

        div()
            .absolute()
            .inset_0()
            .size_full()
            .flex()
            .justify_center()
            .pt(gpui::px(96.0))
            .bg(modal_scrim())
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.hide_modal(window, cx);
                }),
            )
            .child(panel)
            .into_any_element()
    }
}
