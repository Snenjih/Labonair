// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Outer insets of the framed (Comfortable) layout as pure functions.
//!
//! Comfortable cards keep a gap to their *horizontal* neighbours, but never
//! inset vertically from the titlebar / statusbar: top and bottom edges sit
//! exactly where the flat Compact layout docks them. (A vertical gap here
//! used to shrink every card and push the titlebar zone.)

/// Per-edge insets in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// Which edge a dock is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockEdge {
    Left,
    Right,
    Bottom,
}

/// Padding around the pane tree.
pub fn pane_area_insets(framed: bool, gap: f32) -> Insets {
    let x = if framed { gap } else { 0.0 };
    Insets {
        left: x,
        right: x,
        ..Insets::default()
    }
}

/// Margin around a dock card. Outer sides get the gap; the inner side is
/// supplied by the pane area (side docks) or by a top margin (bottom dock,
/// which separates it from the panes above).
pub fn dock_margins(framed: bool, gap: f32, edge: DockEdge) -> Insets {
    if !framed {
        return Insets::default();
    }
    match edge {
        DockEdge::Left => Insets {
            left: gap,
            ..Insets::default()
        },
        DockEdge::Right => Insets {
            right: gap,
            ..Insets::default()
        },
        DockEdge::Bottom => Insets {
            left: gap,
            right: gap,
            top: gap,
            bottom: 0.0,
        },
    }
}

/// Safety margin between a terminal pane's edge and its text (Comfortable
/// only; scales with the UI density).
pub const TERMINAL_PADDING: f32 = 9.0;

pub fn terminal_padding(framed: bool, density: f32) -> f32 {
    if framed {
        TERMINAL_PADDING * density
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDGES: [DockEdge; 3] = [DockEdge::Left, DockEdge::Right, DockEdge::Bottom];

    #[test]
    fn side_docks_and_pane_area_have_no_vertical_inset_in_either_layout() {
        for gap in [1.0, 8.0, 9.2] {
            for framed in [false, true] {
                let area = pane_area_insets(framed, gap);
                assert_eq!((area.top, area.bottom), (0.0, 0.0));
                for edge in [DockEdge::Left, DockEdge::Right] {
                    let m = dock_margins(framed, gap, edge);
                    assert_eq!((m.top, m.bottom), (0.0, 0.0));
                }
                // The bottom dock touches the statusbar in both layouts.
                assert_eq!(dock_margins(framed, gap, DockEdge::Bottom).bottom, 0.0);
            }
        }
    }

    #[test]
    fn comfortable_keeps_horizontal_gaps_and_classic_has_none() {
        assert_eq!(pane_area_insets(true, 8.0).left, 8.0);
        assert_eq!(dock_margins(true, 8.0, DockEdge::Left).left, 8.0);
        assert_eq!(dock_margins(true, 8.0, DockEdge::Right).right, 8.0);
        assert_eq!(dock_margins(true, 8.0, DockEdge::Bottom).top, 8.0);
        for edge in EDGES {
            assert_eq!(dock_margins(false, 8.0, edge), Insets::default());
        }
        assert_eq!(pane_area_insets(false, 8.0), Insets::default());
    }

    #[test]
    fn terminal_padding_is_comfortable_only_and_density_scaled() {
        assert_eq!(terminal_padding(false, 1.15), 0.0);
        assert!((8.0..=10.0).contains(&terminal_padding(true, 1.0)));
        assert!(terminal_padding(true, 1.15) > terminal_padding(true, 0.85));
    }
}
