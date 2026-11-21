// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! File and folder glyphs use a swappable theme with two levels of indirection:
//!
//! 1. `file_stems` / `file_suffixes` map a file name or extension to an
//!    **icon key** (`"rust"`, `"typescript"`, …);
//! 2. `file_icons` maps an icon key to an **asset path**
//!    (`"icons/file_icons/rust.svg"`) in the bundled SVG catalog.
//!
//! `directory` / `chevron` (+ optional `named_directory_icons`) are direct
//! asset paths. The built-in "Labonair" theme is generated from the
//! [`DEFAULT_FILE_STEMS`], [`DEFAULT_FILE_SUFFIXES`], and [`DEFAULT_FILE_ICONS`],
//! embedded in `assets/icon_themes/labonair.json`. The built-in Material Icon
//! Theme uses its complete bundled mapping and SVG set. The user-file loader
//! remains an unwired extension adapter.
//!
//! # JSON format ([`IconThemeContent`])
//!
//! ```json
//! {
//!   "name": "Labonair",
//!   "file_stems":    { "Dockerfile": "docker" },
//!   "file_suffixes": { "rs": "rust", "tsx": "react" },
//!   "file_icons":    { "rust": { "path": "icons/file_icons/rust.svg" } },
//!   "named_directory_icons": {
//!     ".github": { "collapsed": "icons/file_icons/folder.svg",
//!                  "expanded":  "icons/file_icons/folder_open.svg" }
//!   },
//!   "directory": { "collapsed": "icons/file_icons/folder.svg",
//!                  "expanded":  "icons/file_icons/folder_open.svg" },
//!   "chevron":   { "collapsed": "icons/file_icons/chevron_right.svg",
//!                  "expanded":  "icons/file_icons/chevron_down.svg" },
//!   "default_file": "default"
//! }
//! ```
//!
//! Lookup order for a file name (see [`IconThemeContent::file_icon_path`]):
//! 1. the whole (lower-cased) file name in `file_stems` then `file_suffixes`;
//! 2. every dot-delimited trailing suffix, **longest first**
//!    (`archive.tar.gz` tries `tar.gz` before `gz`; `.gitignore` tries
//!    `gitignore`);
//! 3. the `default_file` key.
//!
//! The resolved key is looked up in `file_icons`; a missing key falls back to
//! the `default_file` key, then to the literal `icons/file_icons/file.svg` —
//! a user theme can never blank the tree or panic.

use serde::Deserialize;

pub use labonair_theme_tokens::{
    ChevronIcons, DirectoryIcons, Glyph, IconDefinition, IconThemeContent, BUILTIN_GLYPHS,
    BUILTIN_ICON_THEME_ID, BUILTIN_ICON_THEME_NAME,
};

/// The embedded built-in Labonair icon theme. Regenerate after editing the
/// tables below with `REGEN_BUILTIN_ICON_THEME=1 cargo test -p labonair-theme builtin_icon`.
pub const BUILTIN_ICON_THEME_JSON: &str = include_str!("../assets/icon_themes/labonair.json");

/// Stable ID of the second embedded icon theme.
pub const MATERIAL_ICON_THEME_ID: &str = "material-icon-theme";

/// The embedded Material Icon Theme catalog and path mappings.
pub const MATERIAL_ICON_THEME_JSON: &str =
    include_str!("../assets/icon_themes/material-icon-theme.json");

/// Metadata for one selectable icon theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconThemeMeta {
    /// Stable registry ID. Built-in IDs are constants in this module.
    pub id: String,
    /// Display name.
    pub name: String,
    pub builtin: bool,
}

/// An unknown icon-theme id was requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconThemeNotFoundError(pub String);

impl std::fmt::Display for IconThemeNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "icon theme not found: {}", self.0)
    }
}

impl std::error::Error for IconThemeNotFoundError {}

struct RegisteredIconTheme {
    id: String,
    content: IconThemeContent,
    builtin: bool,
}

/// The embedded built-in icon themes. Optional file loading remains available
/// as a future extension adapter, but the current product catalog is static.
pub struct IconThemeRegistry {
    themes: Vec<RegisteredIconTheme>,
}

impl Default for IconThemeRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl IconThemeRegistry {
    /// A registry holding the embedded built-in themes. A broken Labonair
    /// asset falls back to [`IconThemeContent::default`].
    pub fn builtin() -> Self {
        let embedded = |id: &str, json: &str| {
            IconThemeContent::from_json(json).map(|content| RegisteredIconTheme {
                id: id.to_string(),
                content,
                builtin: true,
            })
        };

        let labonair =
            embedded(BUILTIN_ICON_THEME_ID, BUILTIN_ICON_THEME_JSON).unwrap_or_else(|e| {
                eprintln!("labonair-theme: embedded icon theme is invalid ({e}); using tables");
                RegisteredIconTheme {
                    id: BUILTIN_ICON_THEME_ID.to_string(),
                    content: IconThemeContent::default(),
                    builtin: true,
                }
            });
        let mut themes = vec![labonair];
        match embedded(MATERIAL_ICON_THEME_ID, MATERIAL_ICON_THEME_JSON) {
            Ok(material) => themes.push(material),
            Err(e) => eprintln!(
                "labonair-theme: embedded Material Icon Theme is invalid ({e}); skipping it"
            ),
        }
        Self { themes }
    }

    /// Replace the non-built-in themes with everything valid in `dir`.
    ///
    /// This is an extension adapter, not part of the current static catalog.
    /// It accepts a single theme or a family (`{ "name", "author", "themes": [ … ] }`). Malformed / unreadable files
    /// are skipped and returned as warnings; the built-in themes always remain.
    pub fn load_user_icon_themes(&mut self, dir: &std::path::Path) -> Vec<String> {
        self.themes.retain(|t| t.builtin);
        let mut warnings = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return warnings;
        };

        let mut loaded = Vec::new();
        for path in entries.flatten().map(|entry| entry.path()) {
            let is_json = path.extension().is_some_and(|ext| ext == "json");
            let Some(stem) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|_| is_json)
            else {
                continue;
            };
            // A user file may not shadow a built-in id.
            if self.themes.iter().any(|t| t.id.eq_ignore_ascii_case(stem)) {
                continue;
            }
            let parsed = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|raw| parse_user_icon_theme_file(&raw));
            let contents = match parsed {
                Ok(contents) if !contents.is_empty() => contents,
                Ok(_) => {
                    warnings.push(format!("{}: no themes", path.display()));
                    continue;
                }
                Err(e) => {
                    warnings.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };

            let is_family = contents.len() > 1;
            for content in contents {
                if content.name.trim().is_empty() {
                    warnings.push(format!("{}: missing name", path.display()));
                    continue;
                }
                let id = if is_family {
                    format!("{stem}/{}", content.name)
                } else {
                    stem.to_string()
                };
                loaded.push(RegisteredIconTheme {
                    id,
                    content,
                    builtin: false,
                });
            }
        }

        loaded.sort_by_cached_key(|t| t.content.name.to_lowercase());
        self.themes.extend(loaded);
        warnings
    }

    /// Every selectable icon theme, built-in first.
    pub fn list(&self) -> Vec<IconThemeMeta> {
        self.themes
            .iter()
            .map(|t| IconThemeMeta {
                id: t.id.clone(),
                name: t.content.name.clone(),
                builtin: t.builtin,
            })
            .collect()
    }

    /// Whether `id` names a known icon theme.
    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_ok()
    }

    /// Look an icon theme up by its stable id (`""` / `"default"` → built-in).
    pub fn get(&self, id: &str) -> Result<&IconThemeContent, IconThemeNotFoundError> {
        let wanted = if id.is_empty() {
            BUILTIN_ICON_THEME_ID
        } else {
            id
        };
        self.themes
            .iter()
            .find(|t| {
                t.id.eq_ignore_ascii_case(wanted) || t.content.name.eq_ignore_ascii_case(wanted)
            })
            .map(|t| &t.content)
            .ok_or_else(|| IconThemeNotFoundError(wanted.to_string()))
    }

    /// The built-in icon theme — always present.
    pub fn builtin_theme(&self) -> &IconThemeContent {
        let first_builtin = self.themes.iter().find(|t| t.builtin);
        &first_builtin
            .expect("registry always keeps the built-in icon theme")
            .content
    }
}

/// Parse a user icon-theme file: a family wrapper with a non-empty `themes`
/// list, or else a single theme.
fn parse_user_icon_theme_file(raw: &str) -> Result<Vec<IconThemeContent>, String> {
    #[derive(Deserialize)]
    struct Family {
        themes: Vec<IconThemeContent>,
    }
    match serde_json::from_str::<Family>(raw) {
        Ok(family) if !family.themes.is_empty() => Ok(family.themes),
        _ => IconThemeContent::from_json(raw).map(|single| vec![single]),
    }
}

// ─────────────────────────── canonical built-in tables ──────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_icon_theme_json_round_trips() {
        if std::env::var("REGEN_BUILTIN_ICON_THEME").is_ok() {
            let path = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/icon_themes/labonair.json"
            );
            std::fs::write(path, IconThemeContent::default().to_json().unwrap()).unwrap();
        }
        let embedded = IconThemeContent::from_json(BUILTIN_ICON_THEME_JSON).unwrap();
        assert_eq!(embedded, IconThemeContent::default());
    }

    #[test]
    fn every_icon_key_referenced_is_defined_or_falls_back() {
        let t = IconThemeContent::default();
        // Every stem/suffix key resolves to a bundled icon or falls back to
        // the default icon.
        for key in t.file_stems.values().chain(t.file_suffixes.values()) {
            let path = t
                .file_icons
                .get(key)
                .map(|d| d.path.as_str())
                .unwrap_or_else(|| t.file_icons[&t.default_file].path.as_str());
            assert!(
                path.starts_with("icons/file_icons/") && path.ends_with(".svg"),
                "{key}"
            );
        }
    }

    #[test]
    fn resolves_zed_paths_for_representative_files() {
        let t = IconThemeContent::default();
        assert_eq!(t.file_icon_path("main.rs"), "icons/file_icons/rust.svg");
        assert_eq!(t.file_icon_path("App.tsx"), "icons/file_icons/react.svg");
        assert_eq!(t.file_icon_path("server.go"), "icons/file_icons/go.svg");
        assert_eq!(t.file_icon_path("Cargo.toml"), "icons/file_icons/toml.svg");
        assert_eq!(t.file_icon_path("styles.css"), "icons/file_icons/css.svg");
        assert_eq!(
            t.file_icon_path("Dockerfile"),
            "icons/file_icons/docker.svg"
        );
    }

    #[test]
    fn whole_name_beats_suffix_and_case_folds() {
        let t = IconThemeContent::default();
        // `.dockerignore` is a stem → docker, not the `default` fallback.
        assert_eq!(
            t.file_icon_path(".dockerignore"),
            "icons/file_icons/docker.svg"
        );
        assert_eq!(
            t.file_icon_path(".DOCKERIGNORE"),
            "icons/file_icons/docker.svg"
        );
        // `.gitignore` → `gitignore` suffix → `vcs`.
        assert_eq!(t.file_icon_path(".gitignore"), "icons/file_icons/git.svg");
    }

    #[test]
    fn longest_suffix_wins() {
        let t = IconThemeContent::default();
        // `docker-compose.yml` is a whole-name suffix entry → docker, beating
        // the trailing `yml` → yaml.
        assert_eq!(
            t.file_icon_path("docker-compose.yml"),
            "icons/file_icons/docker.svg"
        );
        assert_eq!(t.file_icon_path("app.yml"), "icons/file_icons/yaml.svg");
    }

    #[test]
    fn unknown_falls_back_to_default_file_icon() {
        let t = IconThemeContent::default();
        assert_eq!(t.file_icon_path("mystery.qqq"), "icons/file_icons/file.svg");
        assert_eq!(t.file_icon_path("noextension"), "icons/file_icons/file.svg");
        assert_eq!(
            t.directory_icon_path("src", false),
            "icons/file_icons/folder.svg"
        );
        assert_eq!(
            t.directory_icon_path("src", true),
            "icons/file_icons/folder_open.svg"
        );
        assert_eq!(
            t.chevron_icon_path(false),
            "icons/file_icons/chevron_right.svg"
        );
        assert_eq!(
            t.chevron_icon_path(true),
            "icons/file_icons/chevron_down.svg"
        );
    }

    #[test]
    fn named_directory_icons_win_when_present() {
        let mut t = IconThemeContent::default();
        t.named_directory_icons.insert(
            ".github".to_string(),
            DirectoryIcons {
                collapsed: "icons/file_icons/github.svg".to_string(),
                expanded: "icons/file_icons/github.svg".to_string(),
            },
        );
        assert_eq!(
            t.directory_icon_path(".github", false),
            "icons/file_icons/github.svg"
        );
        assert_eq!(
            t.directory_icon_path("src", false),
            "icons/file_icons/folder.svg"
        );
    }

    #[test]
    fn registry_loads_user_themes_and_skips_broken() {
        let dir = std::env::temp_dir().join(format!("labonair-icon-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mono.json"),
            r#"{ "name": "Mono", "file_suffixes": { "rs": "binary" },
                "file_icons": { "binary": { "path": "icons/file_icons/file.svg" } },
                "directory": { "collapsed": "icons/file_icons/folder.svg", "expanded": "icons/file_icons/folder_open.svg" },
                "chevron": { "collapsed": "icons/file_icons/chevron_right.svg", "expanded": "icons/file_icons/chevron_down.svg" },
                "default_file": "default" }"#,
        )
        .unwrap();
        std::fs::write(dir.join("broken.json"), "{ not json").unwrap();

        let mut reg = IconThemeRegistry::builtin();
        let warnings = reg.load_user_icon_themes(&dir);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("broken.json"));
        assert!(reg.contains("mono"));
        assert!(reg.contains("default"));
        assert_eq!(
            reg.get("mono").unwrap().file_icon_path("main.rs"),
            "icons/file_icons/file.svg"
        );

        std::fs::remove_file(dir.join("mono.json")).unwrap();
        reg.load_user_icon_themes(&dir);
        assert!(!reg.contains("mono"));
        assert!(reg.contains("default"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_loads_a_family_file() {
        let dir = std::env::temp_dir().join(format!("labonair-icon-fam-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pack.json"),
            r#"{ "name": "Pack", "author": "x", "themes": [
                { "name": "Pack Dark", "directory": { "collapsed": "icons/file_icons/folder.svg", "expanded": "icons/file_icons/folder_open.svg" },
                  "chevron": { "collapsed": "icons/file_icons/chevron_right.svg", "expanded": "icons/file_icons/chevron_down.svg" },
                  "default_file": "default" },
                { "name": "Pack Light", "directory": { "collapsed": "icons/file_icons/folder.svg", "expanded": "icons/file_icons/folder_open.svg" },
                  "chevron": { "collapsed": "icons/file_icons/chevron_right.svg", "expanded": "icons/file_icons/chevron_down.svg" },
                  "default_file": "default" }
            ] }"#,
        )
        .unwrap();
        let mut reg = IconThemeRegistry::builtin();
        reg.load_user_icon_themes(&dir);
        assert!(reg.contains("pack/Pack Dark"));
        assert!(reg.contains("pack/Pack Light"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_id_is_typed_error() {
        let reg = IconThemeRegistry::builtin();
        let err = reg.get("nope").unwrap_err();
        assert_eq!(err, IconThemeNotFoundError("nope".to_string()));
        assert!(err.to_string().contains("icon theme not found"));
    }
}
