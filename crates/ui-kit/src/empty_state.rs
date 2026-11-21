// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Shared centered empty-state content.

use gpui::{div, Div, ParentElement, SharedString, Styled};

use crate::Palette;

/// Render a centered empty-state message using the shared muted text color.
/// The caller controls the available height and any feature-specific type size.
pub fn empty_state(message: impl Into<SharedString>, palette: Palette) -> Div {
    let message = message.into();
    div()
        .flex()
        .w_full()
        .items_center()
        .justify_center()
        .text_color(palette.muted)
        .child(message)
}
