// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! `Select` / `EnumDropdown` — a trigger showing the current option plus the
//! anchored list of the alternatives.
//!
//! The trigger and list use Labonair theme tokens, and the popup shares the
//! same viewport-aware positioner as context menus and rich-content popovers.
//! It is designed from the pinned reference's observable control states while
//! keeping option ownership and selection with the calling feature.
//!
//! `gpui-component` ships a `select` module, but it styles itself from
//! *its own* `cx.theme()` global — which the app never syncs to
//! `labonair-theme` — so wrapping it would silently bypass our tokens
//! (Critical Rule 3). The trigger/list pair below is token-bound instead.
//!
//! The open/closed state stays with the caller (it already does — settings-ui
//! keeps a `dropdown: Option<SelectMenu>`), so this is deliberately two
//! functions rather than one stateful element:
//!
//! ```ignore
//! // in the row:
//! select_trigger("sel-font", c, current_label, is_open)
//!     .on_click(cx.listener(|this, ev: &ClickEvent, _w, cx| this.open_menu(ev.position(), cx)))
//! // once, at the view's top level (so it is not clipped by the scroll area):
//! select_popover(
//!     SelectPopoverAnchor::new("font", menu.at, list_state),
//!     c, &menu.options, &current, dismiss, on_select,
//! )
//! ```

use std::rc::Rc;

use gpui::AppContext as _;
use gpui::{
    deferred, div, list, prelude::FluentBuilder, px, AnyElement, App, ClickEvent, Context, Div,
    ElementId, Entity, FontWeight, InteractiveElement, IntoElement, ListAlignment,
    ListSizingBehavior, ListState, ParentElement, Pixels, Point, RenderOnce, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::input::InputEvent;

use crate::icon::IconName;
use crate::palette::Palette;
use crate::popover::anchored_popup;
use crate::presence::{closing_guard, enter_or_popup};
use crate::search_field::{search_field, search_input};
use crate::text_field::InputState;
use crate::DISABLED_OPACITY;

/// One `(token, label)` pair — the token is what gets stored, the label what is
/// shown.
pub type SelectOption = (SharedString, SharedString);

/// Menus with more than this many options get a type-to-filter search input.
pub const SELECT_SEARCH_THRESHOLD: usize = 100;

/// Indices of `options` whose label contains `query` (case-insensitive).
/// Prefix matches come first; otherwise the original order is kept. An empty
/// query keeps everything.
pub fn filter_options(options: &[SelectOption], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (0..options.len()).collect();
    }
    let (mut prefix, mut rest) = (Vec::new(), Vec::new());
    for (i, (_, label)) in options.iter().enumerate() {
        let l = label.to_lowercase();
        if l.starts_with(&q) {
            prefix.push(i);
        } else if l.contains(&q) {
            rest.push(i);
        }
    }
    prefix.extend(rest);
    prefix
}

/// The label to show for `value`, or `None` when it matches no option.
///
/// Pure — lets call sites (and tests) resolve the trigger text without
/// rendering.
pub fn selected_label<'a>(options: &'a [SelectOption], value: &str) -> Option<&'a SharedString> {
    options
        .iter()
        .find(|(token, _)| token.as_ref() == value)
        .map(|(_, label)| label)
}

/// The closed control: current label + chevron, accent border while `open`.
pub fn select_trigger(
    id: impl Into<ElementId>,
    c: Palette,
    label: impl Into<SharedString>,
    open: bool,
) -> Stateful<Div> {
    select_trigger_with_state(id, c, label, open, false)
}

/// The inert visual state of a select trigger. Disabled triggers preserve the
/// normal control geometry while dropping pointer affordance and keyboard
/// focus.
pub fn select_trigger_disabled(
    id: impl Into<ElementId>,
    c: Palette,
    label: impl Into<SharedString>,
) -> Stateful<Div> {
    select_trigger_with_state(id, c, label, false, true)
}

/// Per-owner state required to render one anchored, virtualized select list.
#[derive(Clone)]
pub struct SelectPopoverAnchor {
    id: SharedString,
    position: Point<Pixels>,
    list_state: ListState,
}

impl SelectPopoverAnchor {
    pub fn new(
        id: impl Into<SharedString>,
        position: Point<Pixels>,
        list_state: ListState,
    ) -> Self {
        Self {
            id: id.into(),
            position,
            list_state,
        }
    }
}

fn select_trigger_with_state(
    id: impl Into<ElementId>,
    c: Palette,
    label: impl Into<SharedString>,
    open: bool,
    disabled: bool,
) -> Stateful<Div> {
    let mut trigger = div()
        .id(id)
        .min_w(c.space(160.0))
        .h(c.control_space(32.0))
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px(c.control_space(12.0))
        .rounded(px(c.radius.md))
        .border_1()
        .border_color(if open { c.field_ring } else { c.input })
        .bg(if disabled { c.muted_bg } else { c.field_bg })
        .text_color(if disabled { c.muted } else { c.fg })
        .text_size(px(13.0 * c.ui_font_scale))
        .child(label.into())
        .child(IconName::ChevronDown.svg(c.muted).size(px(14.0)));
    trigger = if disabled {
        trigger
            .tab_index(-1)
            .cursor_default()
            .opacity(DISABLED_OPACITY)
    } else {
        trigger
            .cursor_pointer()
            .tab_index(0)
            .focus(|s| s.border_color(c.field_ring))
    };
    trigger
}

/// The open options list, anchored at `anchor` (window coordinates — pass the
/// click position or the trigger's bottom-left) with outside-press dismissal.
///
/// `deferred` + shared window-anchored placement keeps the list out of ancestor
/// clipping and preserves a consistent margin near each viewport edge. The
/// caller owns `list_state`; rows are rendered virtually and the viewport is
/// capped at 320 logical pixels.
pub fn select_popover(
    anchor: SelectPopoverAnchor,
    c: Palette,
    options: &[SelectOption],
    selected: &str,
    dismiss: impl Fn(&mut Window, &mut App) + 'static,
    on_select: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
) -> AnyElement {
    select_popover_with_exit(anchor, c, options, selected, false, dismiss, on_select)
}

/// [`select_popover`] with an exit state: while `closing` the list plays the
/// exit fade and ignores pointer input. Drive `closing` from a
/// [`crate::Presence`]; see [`crate::presence`] for the wiring.
pub fn select_popover_with_exit(
    anchor: SelectPopoverAnchor,
    c: Palette,
    options: &[SelectOption],
    selected: &str,
    closing: bool,
    dismiss: impl Fn(&mut Window, &mut App) + 'static,
    on_select: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let SelectPopoverAnchor {
        id,
        position,
        list_state,
    } = anchor;
    let dismiss = Rc::new(dismiss);
    let on_select = Rc::new(on_select);

    let selected = SharedString::from(selected.to_owned());
    let row_dismiss = dismiss.clone();
    let row_on_select = on_select.clone();
    let row_id = id.clone();
    let rows = options.to_vec();
    let list_height = c
        .control_space(28.0 * options.len() as f32)
        .min(c.space(320.0));
    if options.len() > SELECT_SEARCH_THRESHOLD {
        return SearchableSelect {
            id,
            position,
            c,
            options: options.to_vec(),
            selected,
            closing,
            dismiss,
            on_select,
        }
        .into_any_element();
    }
    let list = list(list_state, move |i, _, _| {
        let (token, label) = rows[i].clone();
        let on = token == selected;
        option_row(
            &row_id,
            i,
            c,
            token,
            label,
            on,
            false,
            row_dismiss.clone(),
            row_on_select.clone(),
        )
    })
    .with_sizing_behavior(ListSizingBehavior::Infer)
    .h(list_height)
    .max_h(c.space(320.0))
    .w_full();

    popup_shell(id, position, c, closing, dismiss, list)
}

type DismissFn = Rc<dyn Fn(&mut Window, &mut App)>;
type SelectFn = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;

#[allow(clippy::too_many_arguments)]
fn option_row(
    row_id: &SharedString,
    i: usize,
    c: Palette,
    token: SharedString,
    label: SharedString,
    on: bool,
    highlighted: bool,
    dismiss: DismissFn,
    on_select: SelectFn,
) -> AnyElement {
    div()
        .id(SharedString::from(format!("{row_id}-opt-{i}")))
        .w_full()
        .h(c.control_space(28.0))
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px(c.control_space(8.0))
        .rounded(px(c.radius.md))
        .text_size(px(13.0 * c.ui_font_scale))
        .text_color(if on { c.fg } else { c.card_fg })
        .cursor_pointer()
        .when(on, |d| d.font_weight(FontWeight::SEMIBOLD))
        .when(highlighted, |d| d.bg(c.accent))
        .hover(|s| s.bg(c.accent))
        .child(label)
        .when(on, |d| d.child(IconName::Check.svg(c.fg).size(px(14.0))))
        .on_click(move |_: &ClickEvent, w, cx| {
            on_select(&token, w, cx);
            dismiss(w, cx);
            cx.stop_propagation();
        })
        .into_any_element()
}

fn popup_shell(
    id: SharedString,
    position: Point<Pixels>,
    c: Palette,
    closing: bool,
    dismiss: DismissFn,
    body: impl IntoElement,
) -> AnyElement {
    let outside_dismiss = dismiss;
    let list = anchored_popup(position, c).child(enter_or_popup(
        div()
            .id(SharedString::from(format!("{id}-list")))
            .occlude()
            .min_w(c.space(180.0))
            .flex()
            .flex_col()
            .p(c.space(4.0))
            .rounded(px(c.radius.lg))
            .bg(c.popover)
            .border_1()
            .border_color(c.border)
            .shadow(c.popover_shadow())
            .child(body)
            .on_mouse_down_out(move |_, window, cx| outside_dismiss(window, cx))
            .when(closing, |card| card.child(closing_guard())),
        "select-fade",
        c,
        closing,
    ));
    deferred(list).with_priority(200).into_any_element()
}

/// Filter input + list + keyboard highlight, kept across frames via keyed state.
struct SearchState {
    input: Entity<InputState>,
    list: ListState,
    highlighted: usize,
    focused: bool,
}

/// A select list with a search input (see [`SELECT_SEARCH_THRESHOLD`]).
#[derive(IntoElement)]
struct SearchableSelect {
    id: SharedString,
    position: Point<Pixels>,
    c: Palette,
    options: Vec<SelectOption>,
    selected: SharedString,
    closing: bool,
    dismiss: DismissFn,
    on_select: SelectFn,
}

impl RenderOnce for SearchableSelect {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            position,
            c,
            options,
            selected,
            closing,
            dismiss,
            on_select,
        } = self;
        let state = window.use_keyed_state(
            SharedString::from(format!("{id}-search")),
            cx,
            |window, cx: &mut Context<SearchState>| {
                let input = cx.new(|cx| {
                    let s = InputState::new(window, cx);
                    s.focus(window, cx);
                    s
                });
                cx.subscribe(&input, |this, _, ev: &InputEvent, cx| {
                    if matches!(ev, InputEvent::Change) {
                        this.highlighted = 0;
                    }
                    cx.notify();
                })
                .detach();
                SearchState {
                    input,
                    list: ListState::new(0, ListAlignment::Top, px(320.0)),
                    highlighted: 0,
                    focused: true,
                }
            },
        );
        let (input, list_state, highlighted) = {
            let s = state.read(cx);
            (s.input.clone(), s.list.clone(), s.highlighted)
        };
        let query = input.read(cx).value();
        let matches = Rc::new(filter_options(&options, &query));
        if list_state.item_count() != matches.len() {
            list_state.reset(matches.len());
        }
        let highlighted = highlighted.min(matches.len().saturating_sub(1));

        let rows: Vec<SelectOption> = matches.iter().map(|&i| options[i].clone()).collect();
        let (row_id, row_dismiss, row_select) = (id.clone(), dismiss.clone(), on_select.clone());
        let body = if rows.is_empty() {
            div()
                .h(c.control_space(28.0))
                .flex()
                .items_center()
                .px(c.control_space(8.0))
                .text_size(px(13.0 * c.ui_font_scale))
                .text_color(c.muted)
                .child("No matches")
                .into_any_element()
        } else {
            let height = c
                .control_space(28.0 * rows.len() as f32)
                .min(c.space(320.0));
            list(list_state.clone(), move |i, _, _| {
                let (token, label) = rows[i].clone();
                let on = token == selected;
                option_row(
                    &row_id,
                    i,
                    c,
                    token,
                    label,
                    on,
                    i == highlighted,
                    row_dismiss.clone(),
                    row_select.clone(),
                )
            })
            .with_sizing_behavior(ListSizingBehavior::Infer)
            .h(height)
            .max_h(c.space(320.0))
            .w_full()
            .into_any_element()
        };

        let nav_state = state.clone();
        let nav_matches = matches.clone();
        let nav_options = options.clone();
        let (nav_dismiss, nav_select) = (dismiss.clone(), on_select.clone());
        let nav_list = list_state.clone();
        // Fixed width: an input inside a content-sized popup has no definite
        // width, which left the caret floating mid-field.
        let content = div()
            .w(c.space(280.0))
            .flex()
            .flex_col()
            .gap(c.space(4.0))
            .capture_key_down(move |ev, window, cx| {
                let n = nav_matches.len();
                let key = ev.keystroke.key.as_str();
                let step = |delta: isize, cx: &mut App| {
                    if n == 0 {
                        return;
                    }
                    nav_state.update(cx, |s, cx| {
                        s.highlighted =
                            (s.highlighted as isize + delta).rem_euclid(n as isize) as usize;
                        nav_list.scroll_to_reveal_item(s.highlighted);
                        cx.notify();
                    });
                };
                match key {
                    "down" => step(1, cx),
                    "up" => step(-1, cx),
                    "enter" => {
                        let at = nav_state.read(cx).highlighted.min(n.saturating_sub(1));
                        if let Some(&i) = nav_matches.get(at) {
                            nav_select(&nav_options[i].0, window, cx);
                            nav_dismiss(window, cx);
                        }
                    }
                    "escape" => nav_dismiss(window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            })
            .child(search_field(
                SharedString::from(format!("{id}-search-field")),
                c,
                state.read(cx).focused,
                search_input(&input, c),
                None,
            ))
            .child(body);
        popup_shell(id, position, c, closing, dismiss, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_palette;

    fn opts() -> Vec<SelectOption> {
        vec![
            ("block".into(), "Block".into()),
            ("bar".into(), "Bar".into()),
        ]
    }

    #[test]
    fn filter_is_case_insensitive_prefix_first() {
        let o: Vec<SelectOption> = ["Fira Code", "Menlo", "Source Code Pro", "code"]
            .iter()
            .map(|n| ((*n).into(), (*n).into()))
            .collect();
        assert_eq!(filter_options(&o, ""), vec![0, 1, 2, 3]);
        assert_eq!(filter_options(&o, "CODE"), vec![3, 0, 2]);
        assert!(filter_options(&o, "zzz").is_empty());
    }

    #[test]
    fn resolves_the_selected_label() {
        let o = opts();
        assert_eq!(
            selected_label(&o, "bar").map(SharedString::as_ref),
            Some("Bar")
        );
        assert!(selected_label(&o, "underline").is_none());
        assert!(selected_label(&[], "bar").is_none());
    }

    #[test]
    fn trigger_and_popover_build_in_both_states() {
        let c = test_palette();
        for open in [true, false] {
            let _ = select_trigger("sel", c, "Block", open);
        }
        let _ = select_trigger_disabled("sel-disabled", c, "Block");
        let _ = select_popover(
            SelectPopoverAnchor::new(
                "sel",
                Point::default(),
                ListState::new(opts().len(), gpui::ListAlignment::Top, px(320.0)),
            ),
            c,
            &opts(),
            "bar",
            |_, _| {},
            |_, _, _| {},
        );
        // An empty option list must not panic either.
        let _ = select_popover(
            SelectPopoverAnchor::new(
                "sel",
                Point::default(),
                ListState::new(0, gpui::ListAlignment::Top, px(320.0)),
            ),
            c,
            &[],
            "",
            |_, _| {},
            |_, _, _| {},
        );
    }

    struct SearchHost {
        opts: Vec<SelectOption>,
        picked: std::rc::Rc<std::cell::RefCell<Option<String>>>,
    }

    impl gpui::Render for SearchHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let picked = self.picked.clone();
            div().size_full().child(select_popover(
                SelectPopoverAnchor::new(
                    "sel",
                    Point::new(px(20.0), px(20.0)),
                    ListState::new(0, gpui::ListAlignment::Top, px(320.0)),
                ),
                test_palette(),
                &self.opts,
                "",
                |_, _| {},
                move |t, _, _| *picked.borrow_mut() = Some(t.to_string()),
            ))
        }
    }

    #[gpui::test]
    fn searchable_select_accepts_typing_and_filters(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let picked = std::rc::Rc::new(std::cell::RefCell::new(None));
        let opts: Vec<SelectOption> = (0..150)
            .map(|i| (format!("t{i}").into(), format!("Item {i}").into()))
            .collect();
        let host = picked.clone();
        let (_root, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|_| SearchHost { opts, picked: host });
            gpui_component::Root::new(view, window, cx)
        });
        cx.run_until_parked();
        cx.simulate_input("Item 137");
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        assert_eq!(picked.borrow().as_deref(), Some("t137"));
    }
}
