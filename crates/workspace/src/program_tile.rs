// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Tab tile for a terminal's foreground program. Tiles use abstract glyphs and
//! program-associated colors without third-party logos.

use labonair_terminal::resume::normalize_process_name;
use labonair_ui_kit::{IconName, TabTileSpec};

const WHITE: u32 = 0xffffff;

/// Interactive shells never get a program tile (the plain terminal tile does).
fn is_shell(name: &str) -> bool {
    matches!(
        name,
        "zsh"
            | "bash"
            | "fish"
            | "sh"
            | "dash"
            | "nu"
            | "pwsh"
            | "elvish"
            | "xonsh"
            | "tcsh"
            | "csh"
            | "ksh"
    )
}

/// Glyph, tile colour and glyph colour for a known program (`None` if unknown).
fn known_look(name: &str) -> Option<(IconName, u32, u32)> {
    use IconName::*;
    let look = match name {
        "node" => (Hexagon, 0x5fa04e, WHITE),
        "deno" => (Code, 0x1b1b1b, WHITE),
        "python" | "python3" | "ipython" => (SnakeBlocks, 0x3776ab, 0xffd43b),
        "go" => (Code, 0x00add8, WHITE),
        "cargo" | "rustc" => (Code, 0x2b2b2b, 0xf4a261),
        "ruby" | "irb" => (Code, 0xcc342d, WHITE),
        "lua" => (Code, 0x2c2d72, WHITE),
        "php" => (Code, 0x777bb4, WHITE),
        "swift" => (Code, 0xf05138, WHITE),
        "kotlin" => (Code, 0x7f52ff, WHITE),
        "npm" => (Box, 0xcb3837, WHITE),
        "pnpm" => (Box, 0xf69220, WHITE),
        "yarn" => (Box, 0x2c8ebb, WHITE),
        "lazygit" => (Diff, 0x3f8f4f, WHITE),
        "git" | "tig" => (GitBranch, 0xf05032, WHITE),
        "vim" | "vi" | "view" => (Pencil, 0x019733, WHITE),
        "nvim" => (Pencil, 0x2b7a3d, WHITE),
        "hx" => (Pencil, 0x281733, 0xc7a3f5),
        "emacs" => (Pencil, 0x7f5ab6, WHITE),
        "top" | "htop" | "btop" => (Activity, 0x1f6f43, 0xa7f3c4),
        "ssh" | "mosh" => (Public, 0x5b4bd6, WHITE),
        "docker" => (Container, 0x2496ed, WHITE),
        "psql" | "mysql" | "redis-cli" => (DatabaseZap, 0x336791, WHITE),
        "tmux" => (Split, 0x1bb91f, WHITE),
        "make" => (Cpu, 0x6b7280, WHITE),
        "less" | "man" => (FileCode, 0x475569, WHITE),
        _ => return None,
    };
    Some(look)
}

/// Tile colour for a program without a dedicated look: a deterministic pick
/// derived from the name so a given program always keeps its colour.
fn fallback_colour(name: &str) -> u32 {
    const SWATCHES: [u32; 6] = [0x7457d6, 0x2b7de9, 0x12a08c, 0xd98a2b, 0xc03e7a, 0x4f8a3d];
    let seed = name.bytes().fold(5381usize, |acc, b| {
        acc.wrapping_mul(33).wrapping_add(b as usize)
    });
    SWATCHES[seed % SWATCHES.len()]
}

/// Tile for a foreground process, or `None` for a shell / an empty name (the
/// plain terminal tile applies then).
pub(crate) fn program_tile(process: &str) -> Option<TabTileSpec> {
    let name = normalize_process_name(process.trim_start_matches('-'));
    if name.is_empty() || is_shell(&name) {
        return None;
    }
    // Bun's tile is light, so it needs the outline to stay visible.
    if name == "bun" {
        return Some(TabTileSpec::program(IconName::Code, 0xfbf0df, 0x3b2a20).outlined());
    }
    Some(match known_look(&name) {
        Some((glyph, bg, fg)) => TabTileSpec::program(glyph, bg, fg),
        None => TabTileSpec::program(IconName::Terminal, fallback_colour(&name), WHITE),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shells_and_empty_have_no_program_tile() {
        assert_eq!(program_tile("zsh"), None);
        assert_eq!(program_tile("-bash"), None);
        assert_eq!(program_tile("/bin/fish"), None);
        assert_eq!(program_tile(""), None);
    }

    #[test]
    fn known_programs_map_by_normalised_name() {
        let node = program_tile("/usr/local/bin/node").unwrap();
        assert_eq!(node.glyph, IconName::Hexagon);
        assert_eq!(program_tile("python3.exe"), program_tile("python"));
        assert_eq!(program_tile("btop").unwrap().glyph, IconName::Activity);
    }

    #[test]
    fn unknown_programs_get_a_stable_colour() {
        let a = program_tile("frobnicate").unwrap();
        assert_eq!(a, program_tile("frobnicate").unwrap());
        assert_eq!(a.glyph, IconName::Terminal);
    }
}
