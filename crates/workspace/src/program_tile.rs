//! Foreground-program tiles for terminal tabs (Gorex `programs.go`
//! `programOf`). Tiles are own abstract glyphs on brand-ish colours; no
//! third-party logos are shipped (see `docs/architecture.md` §8.19).

use labonair_terminal::resume::normalize_process_name;
use labonair_ui_kit::{IconName, TabTileSpec};

const WHITE: u32 = 0xffffff;

/// `(process names, glyph, tile bg, glyph colour, light tile needs outline)`.
#[allow(clippy::type_complexity)]
const PROGRAMS: &[(&[&str], IconName, u32, u32, bool)] = &[
    (&["node"], IconName::Hexagon, 0x5fa04e, WHITE, false),
    (&["bun"], IconName::Code, 0xfbf0df, 0x3b2a20, true),
    (&["deno"], IconName::Code, 0x1b1b1b, WHITE, false),
    (
        &["python", "python3", "ipython"],
        IconName::SnakeBlocks,
        0x3776ab,
        0xffd43b,
        false,
    ),
    (&["lazygit"], IconName::Diff, 0x3f8f4f, WHITE, false),
    (&["tig", "git"], IconName::GitBranch, 0xf05032, WHITE, false),
    (
        &["vim", "vi", "view"],
        IconName::Pencil,
        0x019733,
        WHITE,
        false,
    ),
    (&["nvim"], IconName::Pencil, 0x2b7a3d, WHITE, false),
    (&["hx"], IconName::Pencil, 0x281733, 0xc7a3f5, false),
    (&["emacs"], IconName::Pencil, 0x7f5ab6, WHITE, false),
    (
        &["htop", "btop", "top"],
        IconName::Activity,
        0x1f6f43,
        0xa7f3c4,
        false,
    ),
    (&["ssh", "mosh"], IconName::Public, 0x5b4bd6, WHITE, false),
    (&["docker"], IconName::Container, 0x2496ed, WHITE, false),
    (&["go"], IconName::Code, 0x00add8, WHITE, false),
    (
        &["cargo", "rustc"],
        IconName::Code,
        0x2b2b2b,
        0xf4a261,
        false,
    ),
    (&["npm"], IconName::Box, 0xcb3837, WHITE, false),
    (&["pnpm"], IconName::Box, 0xf69220, WHITE, false),
    (&["yarn"], IconName::Box, 0x2c8ebb, WHITE, false),
    (&["ruby", "irb"], IconName::Code, 0xcc342d, WHITE, false),
    (&["lua"], IconName::Code, 0x2c2d72, WHITE, false),
    (&["php"], IconName::Code, 0x777bb4, WHITE, false),
    (&["swift"], IconName::Code, 0xf05138, WHITE, false),
    (&["kotlin"], IconName::Code, 0x7f52ff, WHITE, false),
    (
        &["psql", "mysql", "redis-cli"],
        IconName::DatabaseZap,
        0x336791,
        WHITE,
        false,
    ),
    (&["tmux"], IconName::Split, 0x1bb91f, WHITE, false),
    (&["make"], IconName::Cpu, 0x6b7280, WHITE, false),
    (&["less", "man"], IconName::FileCode, 0x475569, WHITE, false),
];

const SHELLS: &[&str] = &[
    "zsh", "bash", "fish", "sh", "dash", "nu", "pwsh", "elvish", "xonsh", "tcsh", "csh", "ksh",
];

/// Colours for programs without an entry, picked by a stable hash of the name.
const HASHED: [u32; 8] = [
    0x6d5bd0, 0x2f80ed, 0x0f9d8a, 0xd9822b, 0xc2417d, 0x3a7d44, 0x8a5a44, 0x4e5d94,
];

/// Tile for a foreground process, or `None` for a shell / an empty name (the
/// plain terminal tile applies then).
pub(crate) fn program_tile(process: &str) -> Option<TabTileSpec> {
    let name = normalize_process_name(process.trim_start_matches('-'));
    if name.is_empty() || SHELLS.contains(&name.as_str()) {
        return None;
    }
    if let Some((_, glyph, bg, fg, outlined)) = PROGRAMS
        .iter()
        .find(|(names, ..)| names.contains(&name.as_str()))
    {
        let tile = TabTileSpec::program(*glyph, *bg, *fg);
        return Some(if *outlined { tile.outlined() } else { tile });
    }
    // FNV-1a, so the colour of an unknown program never changes.
    let hash = name
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    Some(TabTileSpec::program(
        IconName::Terminal,
        HASHED[hash as usize % HASHED.len()],
        WHITE,
    ))
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
