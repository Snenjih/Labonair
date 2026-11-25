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

/// Human-readable name of a known foreground program for the pane header
/// (`None` = keep the shell/OSC title). Agents are named by `Agent::label`.
pub(crate) fn program_label(process: &str, title: &str) -> Option<&'static str> {
    let name = normalize_process_name(process.trim_start_matches('-'));
    let base = base_label(&name)?;
    Some(refine(&name, title).unwrap_or(base))
}

/// Narrower name from the shell title (OSC 0/2), which shells often set to the
/// running command line. Falls back to the base label when it has no hint.
// ponytail: title-word match, only as good as the shell's title; read the
// process args (needs a daemon-protocol change + ADR) if this proves too vague.
fn refine(name: &str, title: &str) -> Option<&'static str> {
    let t = title.to_ascii_lowercase();
    let has = |w: &str| {
        t.split(|c: char| !c.is_alphanumeric() && c != '-')
            .any(|x| x == w)
    };
    Some(match name {
        "git" if has("log") || has("show") || has("blame") => "Git History",
        "git" if has("push") || has("pull") || has("fetch") || has("clone") => "Git Sync",
        "git" if has("rebase") || has("merge") => "Git Merge",
        "npm" | "pnpm" | "yarn" | "bun" if has("dev") || has("start") || has("serve") => {
            "Dev Server"
        }
        "npm" | "pnpm" | "yarn" | "bun" if has("test") => "Tests",
        "npm" | "pnpm" | "yarn" | "bun" if has("build") => "Build",
        "npm" | "pnpm" | "yarn" | "bun" if has("install") || has("add") => "Installing Packages",
        "cargo" if has("test") || has("nextest") => "Tests",
        "cargo" if has("build") || has("check") || has("clippy") => "Build",
        "cargo" if has("run") => "Cargo Run",
        "docker" if has("compose") => "Docker Compose",
        "docker" if has("build") => "Docker Build",
        "python" | "python3" if has("runserver") => "Django Server",
        "python" | "python3" if has("pytest") => "Tests",
        "node" if has("vite") => "Vite",
        "node" if has("next") => "Next.js",
        _ => return None,
    })
}

fn base_label(name: &str) -> Option<&'static str> {
    Some(match name {
        "aider" => "Aider",
        "gemini" => "Gemini CLI",
        "ollama" => "Ollama",
        "terraform" | "tofu" => "Terraform",
        "aws" => "AWS CLI",
        "gcloud" => "Google Cloud CLI",
        "az" => "Azure CLI",
        "gradle" | "gradlew" => "Gradle",
        "mvn" => "Maven",
        "cmake" => "CMake",
        "zed" => "Zed",
        "code" => "VS Code",
        "nc" | "telnet" => "Network Session",
        "vite" => "Vite",
        "pytest" => "Tests",
        "git" => "Git Changes",
        "lazygit" => "Lazygit",
        "tig" => "Git History",
        "gh" => "GitHub CLI",
        "vim" | "vi" | "view" => "Vim",
        "nvim" => "Neovim",
        "hx" => "Helix",
        "emacs" => "Emacs",
        "nano" => "Nano",
        "micro" => "Micro",
        "top" => "Process Monitor",
        "htop" => "Htop",
        "btop" => "Btop",
        "ssh" | "mosh" => "SSH",
        "docker" => "Docker",
        "lazydocker" => "Lazydocker",
        "kubectl" | "k9s" => "Kubernetes",
        "psql" => "PostgreSQL",
        "mysql" => "MySQL",
        "redis-cli" => "Redis",
        "sqlite3" => "SQLite",
        "tmux" => "tmux",
        "make" => "Make",
        "cargo" => "Cargo",
        "rustc" => "Rust Compiler",
        "node" => "Node.js",
        "deno" => "Deno",
        "bun" => "Bun",
        "npm" => "npm",
        "pnpm" => "pnpm",
        "yarn" => "Yarn",
        "python" | "python3" => "Python",
        "ipython" => "IPython",
        "go" => "Go",
        "ruby" | "irb" => "Ruby",
        "less" => "Pager",
        "man" => "Manual",
        "ranger" | "yazi" | "mc" | "nnn" => "File Manager",
        "fzf" => "Fuzzy Finder",
        "watch" => "Watch",
        "tail" => "Log Viewer",
        "ping" => "Ping",
        "curl" | "wget" => "Download",
        "scp" | "sftp" | "rsync" => "File Transfer",
        _ => return None,
    })
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
    fn labels_use_normalised_name() {
        assert_eq!(program_label("/usr/bin/git", ""), Some("Git Changes"));
        assert_eq!(program_label("nvim", ""), Some("Neovim"));
        assert_eq!(program_label("zsh", ""), None);
        assert_eq!(program_label("frobnicate", ""), None);
    }

    #[test]
    fn title_refines_label() {
        assert_eq!(
            program_label("git", "git log --oneline"),
            Some("Git History")
        );
        assert_eq!(program_label("pnpm", "pnpm run dev"), Some("Dev Server"));
        assert_eq!(program_label("cargo", "cargo test -p x"), Some("Tests"));
        assert_eq!(program_label("cargo", "~/proj"), Some("Cargo"));
    }

    #[test]
    fn unknown_programs_get_a_stable_colour() {
        let a = program_tile("frobnicate").unwrap();
        assert_eq!(a, program_tile("frobnicate").unwrap());
        assert_eq!(a.glyph, IconName::Terminal);
    }
}
