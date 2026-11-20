// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Flex-layout shorthands.
//!
//! `div().flex().flex_col()` and `div().flex().flex_row().items_center()` are
//! common view patterns. These helpers keep them consistent across feature
//! crates and alongside the rest of the design system.
//!
//! ```ignore
//! v_stack().gap_2().child(header).child(body)
//! h_stack().gap_1().child(icon).child(label)
//! ```

use gpui::{div, Div, Styled};

/// A vertical flex column.
pub fn v_stack() -> Div {
    div().flex().flex_col()
}

/// A horizontal flex row, vertically centred (the shape every
/// icon-next-to-label row wants).
pub fn h_stack() -> Div {
    div().flex().flex_row().items_center()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::ParentElement;

    #[test]
    fn stacks_build() {
        let _ = v_stack().child("a");
        let _ = h_stack().child("a");
    }
}
