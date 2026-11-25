// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Model entity behind the forwards statusbar item.

use gpui::{Context, EventEmitter};
use labonair_forwards::ForwardsUiEvent;

use crate::{primary_action, ForwardRow};

/// Holds the displayed rows and keyboard selection; emits typed requests.
#[derive(Default)]
pub struct ForwardsView {
    rows: Vec<ForwardRow>,
    selected: Option<usize>,
    open_requested: bool,
}

impl EventEmitter<ForwardsUiEvent> for ForwardsView {}

impl ForwardsView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rows(&self) -> &[ForwardRow] {
        &self.rows
    }

    /// Ask the statusbar item to open its popover (observed via `notify`).
    pub fn request_open(&mut self, cx: &mut Context<Self>) {
        self.open_requested = true;
        cx.notify();
    }

    /// Consume a pending [`Self::request_open`].
    pub(crate) fn take_open_request(&mut self) -> bool {
        std::mem::take(&mut self.open_requested)
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Replace the snapshot; notifies only when it changed.
    pub fn set_rows(&mut self, rows: Vec<ForwardRow>, cx: &mut Context<Self>) {
        if self.rows == rows {
            return;
        }
        self.rows = rows;
        self.selected = self.selected.filter(|i| *i < self.rows.len());
        cx.notify();
    }

    /// Move the keyboard selection by `delta`, clamped to the rows.
    pub fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        let next = match self.selected {
            None if delta >= 0 => 0,
            None => last,
            Some(i) => (i as isize + delta).clamp(0, last as isize) as usize,
        };
        self.selected = Some(next);
        cx.notify();
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected.take().is_some() {
            cx.notify();
        }
    }

    /// Emit the primary action of the selected row (Enter).
    pub fn activate_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.selected.and_then(|i| self.rows.get(i)) {
            cx.emit(primary_action(row));
        }
    }

    /// Emit a request on behalf of a button.
    pub fn request(&mut self, event: ForwardsUiEvent, cx: &mut Context<Self>) {
        cx.emit(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_rows, rows::tests::info};
    use gpui::{AppContext, TestAppContext};
    use labonair_ssh::{ForwardId, ForwardState};
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn selection_and_events(cx: &mut TestAppContext) {
        let view = cx.new(|_| ForwardsView::new());
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        cx.update(|cx| {
            cx.subscribe(&view, move |_, e: &ForwardsUiEvent, _| {
                sink.borrow_mut().push(e.clone())
            })
            .detach();
        });
        let rows = build_rows(
            &[
                info("a", "h", ForwardState::Active),
                info("b", "h", ForwardState::Stopped),
            ],
            |_| ("h".into(), vec![]),
        );
        view.update(cx, |v, cx| {
            v.set_rows(rows, cx);
            v.move_selection(1, cx);
            v.move_selection(1, cx);
            v.move_selection(5, cx);
            assert_eq!(v.selected(), Some(1));
            v.activate_selected(cx);
            v.request(ForwardsUiEvent::StopAll, cx);
        });
        assert_eq!(
            *seen.borrow(),
            vec![
                ForwardsUiEvent::Restart(ForwardId("b".into())),
                ForwardsUiEvent::StopAll
            ]
        );
    }
}
