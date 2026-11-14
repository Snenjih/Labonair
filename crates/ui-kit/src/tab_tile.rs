//! Gorex-style tab tile: a small rounded square in a program's colour, a light
//! rim, a soft sheen along the top and a centred glyph (`reference/gorex/tabs.go`
//! `drawTile`). The caller maps a tab kind / detected program to a
//! [`TabTileSpec`]; the ui-kit owns only the look.

use gpui::prelude::FluentBuilder as _;
use gpui::{div, linear_color_stop, linear_gradient, px, rgb, Div, Hsla, ParentElement, Styled};

use crate::{IconName, Palette};

/// Colours + glyph of one tile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabTileSpec {
    pub glyph: IconName,
    pub bg: Hsla,
    pub fg: Hsla,
    /// Light tiles (codex) get a thin dark outline so they stay visible.
    pub outlined: bool,
}

/// Fixed program colours (Gorex `programs.go`); the tile art is brand-coloured
/// on purpose and does not follow the theme.
pub mod tile_colors {
    pub const TERMINAL_BG: u32 = 0x1d2420;
    pub const TERMINAL_FG: u32 = 0x5fd38d;
    pub const CLAUDE: u32 = 0xd97757;
    pub const CODEX_BG: u32 = 0xf7f7f7;
    pub const CODEX_FG: u32 = 0x1a1a1a;
    pub const PI: u32 = 0x2b2b3a;
    pub const OPENCODE: u32 = 0x211e1e;
    pub const SSH: u32 = 0x5b4bd6;
    pub const GIT: u32 = 0xf05032;
    pub const GIT_CHANGES: u32 = 0x3f8f4f;
    pub const EDITOR: u32 = 0x2f6fed;
    pub const PREVIEW: u32 = 0x0e9aa7;
    pub const AI_DIFF: u32 = 0x9b5de5;
    pub const SFTP: u32 = 0xd98a1f;
    pub const KEYMAP: u32 = 0x56606e;
    pub const ONBOARDING: u32 = 0xe0a82e;
    pub const WHITE: u32 = 0xffffff;
}

impl TabTileSpec {
    fn solid(glyph: IconName, bg: u32) -> Self {
        Self {
            glyph,
            bg: rgb(bg).into(),
            fg: rgb(tile_colors::WHITE).into(),
            outlined: false,
        }
    }

    pub fn terminal() -> Self {
        Self {
            glyph: IconName::Terminal,
            bg: rgb(tile_colors::TERMINAL_BG).into(),
            fg: rgb(tile_colors::TERMINAL_FG).into(),
            outlined: false,
        }
    }
    pub fn claude() -> Self {
        Self::solid(IconName::AiClaude, tile_colors::CLAUDE)
    }
    pub fn codex() -> Self {
        Self {
            glyph: IconName::AiOpenAi,
            bg: rgb(tile_colors::CODEX_BG).into(),
            fg: rgb(tile_colors::CODEX_FG).into(),
            outlined: true,
        }
    }
    pub fn pi() -> Self {
        Self::solid(IconName::AgentPi, tile_colors::PI)
    }
    pub fn opencode() -> Self {
        Self::solid(IconName::AiOpenCode, tile_colors::OPENCODE)
    }
    pub fn ssh() -> Self {
        Self::solid(IconName::Public, tile_colors::SSH)
    }
    pub fn editor() -> Self {
        Self::solid(IconName::SquarePen, tile_colors::EDITOR)
    }
    pub fn preview() -> Self {
        Self::solid(IconName::Public, tile_colors::PREVIEW)
    }
    pub fn ai_diff() -> Self {
        Self::solid(IconName::Sparkle, tile_colors::AI_DIFF)
    }
    pub fn sftp() -> Self {
        Self::solid(IconName::FolderOpen, tile_colors::SFTP)
    }
    pub fn git_graph() -> Self {
        Self::solid(IconName::GitGraph, tile_colors::GIT)
    }
    pub fn diff() -> Self {
        Self::solid(IconName::Diff, tile_colors::GIT_CHANGES)
    }
    pub fn commit_diff() -> Self {
        Self::solid(IconName::GitBranch, tile_colors::GIT)
    }
    pub fn keymap() -> Self {
        Self::solid(IconName::Keyboard, tile_colors::KEYMAP)
    }
    pub fn onboarding() -> Self {
        Self::solid(IconName::Home, tile_colors::ONBOARDING)
    }

    /// A coloured tile for a detected program (`bg` as `0xRRGGBB`).
    pub fn program(glyph: IconName, bg: u32, fg: u32) -> Self {
        Self {
            glyph,
            bg: rgb(bg).into(),
            fg: rgb(fg).into(),
            outlined: false,
        }
    }

    /// Same tile with another glyph (host-type icon on the SSH tile).
    pub fn with_glyph(mut self, glyph: IconName) -> Self {
        self.glyph = glyph;
        self
    }

    /// Light tile with a dark outline.
    pub fn outlined(mut self) -> Self {
        self.outlined = true;
        self
    }
}

/// Tile edge length and glyph size (Gorex: 11px glyph in a ~18px tile).
pub const TILE_SIZE: f32 = 18.0;
const GLYPH_SIZE: f32 = 11.0;

/// Render a tile.
pub fn tab_tile(spec: TabTileSpec, c: &Palette) -> Div {
    tile_box(spec, c, TILE_SIZE, true)
}

/// Horizontal peek of each tile behind the front one (Gorex `tileStep`).
const TILE_STEP: f32 = 5.0;
/// Height lost per tile behind the front one, top and bottom (Gorex `inset`).
const TILE_INSET: f32 = 0.8;
/// At most this many tiles are stacked.
pub const MAX_STACKED_TILES: usize = 3;

/// Width of a stack of `n` tiles (clamped to [`MAX_STACKED_TILES`]).
pub fn stack_width(n: usize) -> f32 {
    TILE_SIZE + TILE_STEP * (n.clamp(1, MAX_STACKED_TILES) - 1) as f32
}

/// Gorex-style stack: `specs[0]` in front with its glyph, up to two more
/// peeking out behind it, each further back a little shorter. With one spec
/// this is just [`tab_tile`].
pub fn tab_tile_stack(specs: &[TabTileSpec], c: &Palette) -> Div {
    let Some((front, rest)) = specs.split_first() else {
        return div();
    };
    let back = &rest[..rest.len().min(MAX_STACKED_TILES - 1)];
    if back.is_empty() {
        return tab_tile(*front, c);
    }
    let mut stack = div()
        .relative()
        .flex_none()
        .w(px(stack_width(specs.len())))
        .h(px(TILE_SIZE));
    for (i, spec) in back.iter().enumerate().rev() {
        let inset = TILE_INSET * (i + 1) as f32;
        stack = stack.child(
            tile_box(*spec, c, TILE_SIZE - 2.0 * inset, false)
                .absolute()
                .left(px(TILE_STEP * (i + 1) as f32))
                .top(px(inset)),
        );
    }
    stack.child(
        tile_box(*front, c, TILE_SIZE, true)
            .absolute()
            .left_0()
            .top_0(),
    )
}

fn tile_box(spec: TabTileSpec, c: &Palette, height: f32, glyph: bool) -> Div {
    let radius = px(4.5);
    let sheen_from = gpui::hsla(0.0, 0.0, 1.0, 0.18);
    let sheen_to = gpui::hsla(0.0, 0.0, 1.0, 0.0);
    div()
        .relative()
        .flex_none()
        .w(px(TILE_SIZE))
        .h(px(height))
        .flex()
        .items_center()
        .justify_center()
        .rounded(radius)
        .bg(spec.bg)
        .border_1()
        .border_color(if spec.outlined {
            gpui::hsla(0.0, 0.0, 0.0, 0.12)
        } else {
            tile_rim(c)
        })
        .shadow(vec![gpui::BoxShadow {
            color: gpui::hsla(0.0, 0.0, 0.0, 0.18),
            offset: gpui::point(px(0.0), px(0.5)),
            blur_radius: px(1.5),
            spread_radius: px(0.0),
        }])
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(radius)
                .bg(linear_gradient(
                    180.0,
                    linear_color_stop(sheen_from, 0.0),
                    linear_color_stop(sheen_to, 0.6),
                )),
        )
        .when(glyph, |d| {
            d.child(spec.glyph.svg(spec.fg).size(px(GLYPH_SIZE)))
        })
}

/// The light rim around a tile: a faint foreground tint, readable on both
/// light and dark strips.
pub fn tile_rim(c: &Palette) -> Hsla {
    c.fg.opacity(if c.is_dark { 0.22 } else { 0.14 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_palette;

    #[test]
    fn every_spec_builds() {
        let c = test_palette();
        for spec in [
            TabTileSpec::terminal(),
            TabTileSpec::claude(),
            TabTileSpec::codex(),
            TabTileSpec::pi(),
            TabTileSpec::opencode(),
            TabTileSpec::ssh(),
            TabTileSpec::editor(),
            TabTileSpec::preview(),
            TabTileSpec::ai_diff(),
            TabTileSpec::sftp(),
            TabTileSpec::git_graph(),
            TabTileSpec::diff(),
            TabTileSpec::commit_diff(),
            TabTileSpec::keymap(),
            TabTileSpec::onboarding(),
        ] {
            let _ = tab_tile(spec, &c);
        }
    }

    #[test]
    fn stack_is_capped_at_three_tiles() {
        assert_eq!(stack_width(1), TILE_SIZE);
        assert_eq!(stack_width(3), TILE_SIZE + 2.0 * TILE_STEP);
        assert_eq!(stack_width(9), stack_width(3));
        let c = test_palette();
        let specs = vec![TabTileSpec::terminal(); 5];
        let _ = tab_tile_stack(&specs, &c);
        let _ = tab_tile_stack(&[], &c);
    }
}
