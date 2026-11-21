// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Icon-theme data types + the built-in file/folder glyph tables.
//!
//! Moved out of `labonair-theme::icon_theme` in R08-008 so `labonair-ui-kit`
//! can resolve file/folder icons without depending on the Themes feature
//! crate. The runtime registry (`IconThemeRegistry`, user-theme loading)
//! stays in `labonair-theme`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Collapsed / expanded folder glyph **asset paths**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryIcons {
    pub collapsed: String,
    pub expanded: String,
}

/// Collapsed / expanded disclosure-chevron glyph **asset paths**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChevronIcons {
    pub collapsed: String,
    pub expanded: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconDefinition {
    pub path: String,
}

/// A parsed icon-theme document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconThemeContent {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Preserve colors embedded in the theme's SVG assets instead of tinting
    /// them with the Explorer's muted foreground color.
    #[serde(default)]
    pub color_icons: bool,
    /// Whole-file-name → icon key (checked first, case-insensitive).
    #[serde(default)]
    pub file_stems: BTreeMap<String, String>,
    /// Extension (possibly multi-segment, e.g. `tar.gz`) → icon key.
    #[serde(default)]
    pub file_suffixes: BTreeMap<String, String>,
    /// Icon key → asset path.
    #[serde(default)]
    pub file_icons: BTreeMap<String, IconDefinition>,
    /// Per-name directory glyph overrides (e.g. `.github`), keyed lower-case.
    #[serde(default)]
    pub named_directory_icons: BTreeMap<String, DirectoryIcons>,
    pub directory: DirectoryIcons,
    pub chevron: ChevronIcons,
    /// Fallback icon key for a file that matches nothing.
    pub default_file: String,
}

/// Ultimate fallback when even the `default_file` key is absent.
const HARD_FALLBACK_FILE_ICON: &str = "icons/file_icons/file.svg";

impl IconThemeContent {
    /// Parse from JSON.
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("invalid icon theme: {e}"))
    }

    /// Serialize (pretty) — used to regenerate the embedded built-in asset.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| format!("failed to serialize: {e}"))
    }

    /// The icon **key** for a file name, applying the whole-name → longest-suffix
    /// → `default_file` order documented on the module.
    pub fn file_glyph_key(&self, name: &str) -> &str {
        let lower = name.to_ascii_lowercase();
        if let Some(k) = self
            .file_stems
            .get(&lower)
            .or_else(|| self.file_suffixes.get(&lower))
        {
            return k;
        }
        // Progressively shorter suffixes: for "a.b.c" that is "b.c" then "c";
        // for ".gitignore" that is "gitignore".
        let mut rest = lower.as_str();
        while let Some(idx) = rest.find('.') {
            let suffix = &rest[idx + 1..];
            if suffix.is_empty() {
                break;
            }
            if let Some(k) = self
                .file_stems
                .get(suffix)
                .or_else(|| self.file_suffixes.get(suffix))
            {
                return k;
            }
            rest = suffix;
        }
        &self.default_file
    }

    /// The resolved file-icon **asset path** for `name`.
    pub fn file_icon_path(&self, name: &str) -> &str {
        let key = self.file_glyph_key(name);
        self.file_icons
            .get(key)
            .or_else(|| self.file_icons.get(&self.default_file))
            .map(|d| d.path.as_str())
            .unwrap_or(HARD_FALLBACK_FILE_ICON)
    }

    /// The folder-icon **asset path** for a directory named `name`.
    pub fn directory_icon_path(&self, name: &str, expanded: bool) -> &str {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            if let Some(icons) = self
                .named_directory_icons
                .get(&trimmed.to_ascii_lowercase())
            {
                return if expanded {
                    &icons.expanded
                } else {
                    &icons.collapsed
                };
            }
        }
        if expanded {
            &self.directory.expanded
        } else {
            &self.directory.collapsed
        }
    }

    /// The disclosure-chevron **asset path** for the given open/closed state.
    pub fn chevron_icon_path(&self, expanded: bool) -> &str {
        if expanded {
            &self.chevron.expanded
        } else {
            &self.chevron.collapsed
        }
    }
}

impl Default for IconThemeContent {
    /// The built-in "Labonair" icon theme, built from [`BUILTIN_GLYPHS`]. Used when the embedded JSON asset fails to parse.
    fn default() -> Self {
        Self {
            name: BUILTIN_ICON_THEME_NAME.to_string(),
            author: Some(BUILTIN_ICON_THEME_NAME.to_string()),
            color_icons: false,
            file_stems: glyph_names(|g| g.stems),
            file_suffixes: glyph_names(|g| g.suffixes),
            file_icons: BUILTIN_GLYPHS
                .iter()
                .filter_map(|g| {
                    let path = format!("icons/file_icons/{}.svg", g.asset?);
                    Some((g.key.to_string(), IconDefinition { path }))
                })
                .collect(),
            named_directory_icons: BTreeMap::new(),
            directory: DirectoryIcons {
                collapsed: "icons/file_icons/folder.svg".to_string(),
                expanded: "icons/file_icons/folder_open.svg".to_string(),
            },
            chevron: ChevronIcons {
                collapsed: "icons/file_icons/chevron_right.svg".to_string(),
                expanded: "icons/file_icons/chevron_down.svg".to_string(),
            },
            default_file: "default".to_string(),
        }
    }
}

/// Flatten one name list of every [`BUILTIN_GLYPHS`] entry into a
/// name → icon-key map.
fn glyph_names(
    pick: impl Fn(&'static Glyph) -> &'static [&'static str],
) -> BTreeMap<String, String> {
    BUILTIN_GLYPHS
        .iter()
        .flat_map(|g| {
            pick(g)
                .iter()
                .map(move |name| (name.to_string(), g.key.to_string()))
        })
        .collect()
}

/// The built-in icon-theme display name and stable registry id sentinel.
pub const BUILTIN_ICON_THEME_NAME: &str = "Labonair";
/// Stable id of the built-in icon theme (mirrors the theme registry's `"default"`).
pub const BUILTIN_ICON_THEME_ID: &str = "default";

/// One icon key together with the file names that select it and the bundled
/// SVG it renders (`None` = the key has no artwork of its own and resolves to
/// the theme's default file icon).
pub struct Glyph {
    pub key: &'static str,
    /// SVG file stem inside `icons/file_icons/`.
    pub asset: Option<&'static str>,
    /// Whole (lower-case) file names checked before any suffix.
    pub stems: &'static [&'static str],
    /// Extensions or trailing name segments; multi-segment entries (`tar.gz`)
    /// win over their last segment (longest-suffix rule).
    pub suffixes: &'static [&'static str],
}

/// Built-in glyph catalog, ordered by icon key. It is the single source of
/// truth for the embedded `assets/icon_themes/labonair.json` (regenerate via
/// the `builtin_icon_theme_json_round_trips` test with
/// `REGEN_BUILTIN_ICON_THEME=1`).
#[rustfmt::skip]
pub const BUILTIN_GLYPHS: &[Glyph] = &[
    Glyph { key: "astro", asset: Some("astro"), stems: &[], suffixes: &["astro"] },
    Glyph { key: "audio", asset: Some("audio"), stems: &[], suffixes: &["aac", "flac", "m4a", "mka", "mp3", "ogg", "opus", "wav", "wma", "wv"] },
    Glyph { key: "backup", asset: None, stems: &[], suffixes: &["bak"] },
    Glyph { key: "ballerina", asset: Some("ballerina"), stems: &[], suffixes: &["bal"] },
    Glyph { key: "bicep", asset: Some("file"), stems: &[], suffixes: &["bicep"] },
    Glyph { key: "bun", asset: Some("bun"), stems: &[], suffixes: &["lockb"] },
    Glyph { key: "c", asset: Some("c"), stems: &[], suffixes: &["c", "h"] },
    Glyph { key: "cairo", asset: Some("cairo"), stems: &[], suffixes: &["cairo"] },
    Glyph { key: "code", asset: Some("code"), stems: &[], suffixes: &["handlebars", "metadata", "rkt", "scm"] },
    Glyph { key: "coffeescript", asset: Some("coffeescript"), stems: &[], suffixes: &["coffee"] },
    Glyph { key: "cpp", asset: Some("cpp"), stems: &[], suffixes: &["c++", "h++", "cc", "cpp", "cppm", "cxx", "hh", "hpp", "hxx", "inl", "ixx"] },
    Glyph { key: "crystal", asset: Some("file"), stems: &[], suffixes: &["cr", "ecr"] },
    Glyph { key: "csharp", asset: Some("file"), stems: &[], suffixes: &["cs"] },
    Glyph { key: "csproj", asset: Some("file"), stems: &[], suffixes: &["csproj"] },
    Glyph { key: "css", asset: Some("css"), stems: &[], suffixes: &["css", "pcss", "postcss"] },
    Glyph { key: "cue", asset: Some("file"), stems: &[], suffixes: &["cue"] },
    Glyph { key: "dart", asset: Some("dart"), stems: &[], suffixes: &["dart"] },
    Glyph { key: "default", asset: Some("file"), stems: &[], suffixes: &[] },
    Glyph { key: "diff", asset: Some("diff"), stems: &[], suffixes: &["diff"] },
    Glyph { key: "docker", asset: Some("docker"), stems: &["containerfile", "dockerfile", ".dockerignore"], suffixes: &["docker-compose.yml", "docker-compose.yaml", "compose.yml", "compose.yaml"] },
    Glyph { key: "document", asset: Some("book"), stems: &[], suffixes: &["doc", "docx", "mdx", "odp", "ods", "odt", "pdf", "ppt", "pptx", "rtf", "txt", "xls", "xlsx"] },
    Glyph { key: "editorconfig", asset: Some("editorconfig"), stems: &[], suffixes: &["editorconfig"] },
    Glyph { key: "elixir", asset: Some("elixir"), stems: &[], suffixes: &["eex", "ex", "exs", "heex", "leex", "neex"] },
    Glyph { key: "elm", asset: Some("elm"), stems: &[], suffixes: &["elm"] },
    Glyph { key: "erlang", asset: Some("erlang"), stems: &[], suffixes: &["emakefile", "app.src", "erl", "escript", "hrl", "rebar.config", "xrl", "yrl"] },
    Glyph { key: "eslint", asset: Some("eslint"), stems: &[], suffixes: &["eslint.config.cjs", "eslint.config.cts", "eslint.config.js", "eslint.config.mjs", "eslint.config.mts", "eslint.config.ts", "eslintrc", "eslintrc.js", "eslintrc.json"] },
    Glyph { key: "font", asset: Some("font"), stems: &[], suffixes: &["otf", "ttf", "woff", "woff2"] },
    Glyph { key: "fsharp", asset: Some("fsharp"), stems: &[], suffixes: &["fs"] },
    Glyph { key: "fsproj", asset: Some("file"), stems: &[], suffixes: &["fsproj"] },
    Glyph { key: "gitlab", asset: Some("gitlab"), stems: &[], suffixes: &["gitlab-ci.yml", "gitlab-ci.yaml"] },
    Glyph { key: "gleam", asset: Some("gleam"), stems: &[], suffixes: &["gleam"] },
    Glyph { key: "go", asset: Some("go"), stems: &[], suffixes: &["go", "mod", "work"] },
    Glyph { key: "graphql", asset: Some("graphql"), stems: &[], suffixes: &["gql", "graphql", "graphqls"] },
    Glyph { key: "haskell", asset: Some("haskell"), stems: &[], suffixes: &["hs"] },
    Glyph { key: "hcl", asset: Some("hcl"), stems: &[], suffixes: &["hcl"] },
    Glyph { key: "helm", asset: Some("helm"), stems: &[], suffixes: &["helmfile.yaml", "helmfile.yml", "chart.yaml", "chart.yml", "chart.lock", "values.yaml", "values.yml", "requirements.yaml", "requirements.yml", "tpl"] },
    Glyph { key: "heroku", asset: Some("heroku"), stems: &["procfile"], suffixes: &[] },
    Glyph { key: "html", asset: Some("html"), stems: &[], suffixes: &["htm", "html"] },
    Glyph { key: "image", asset: Some("image"), stems: &[], suffixes: &["avif", "bmp", "gif", "heic", "heif", "ico", "j2k", "jfif", "jp2", "jpeg", "jpg", "jxl", "png", "psd", "qoi", "svg", "tiff", "webp"] },
    Glyph { key: "ipynb", asset: Some("jupyter"), stems: &[], suffixes: &["ipynb"] },
    Glyph { key: "java", asset: Some("java"), stems: &[], suffixes: &["java"] },
    Glyph { key: "javascript", asset: Some("javascript"), stems: &[], suffixes: &["cjs", "js", "mjs"] },
    Glyph { key: "json", asset: Some("code"), stems: &[], suffixes: &["json", "jsonc"] },
    Glyph { key: "julia", asset: Some("julia"), stems: &[], suffixes: &["jl"] },
    Glyph { key: "kdl", asset: Some("kdl"), stems: &[], suffixes: &["kdl"] },
    Glyph { key: "kotlin", asset: Some("kotlin"), stems: &[], suffixes: &["kt"] },
    Glyph { key: "lock", asset: Some("lock"), stems: &[], suffixes: &["lock"] },
    Glyph { key: "log", asset: Some("info"), stems: &[], suffixes: &["log"] },
    Glyph { key: "lua", asset: Some("lua"), stems: &[], suffixes: &["lua"] },
    Glyph { key: "luau", asset: Some("luau"), stems: &[], suffixes: &["luau"] },
    Glyph { key: "markdown", asset: Some("book"), stems: &[], suffixes: &["markdown", "md"] },
    Glyph { key: "metal", asset: Some("metal"), stems: &[], suffixes: &["metal"] },
    Glyph { key: "nim", asset: Some("nim"), stems: &[], suffixes: &["nim", "nims", "nimble"] },
    Glyph { key: "nix", asset: Some("nix"), stems: &[], suffixes: &["nix"] },
    Glyph { key: "ocaml", asset: Some("ocaml"), stems: &[], suffixes: &["ml", "mli", "mlx"] },
    Glyph { key: "odin", asset: Some("odin"), stems: &[], suffixes: &["odin"] },
    Glyph { key: "phoenix", asset: Some("phoenix"), stems: &[], suffixes: &[] },
    Glyph { key: "php", asset: Some("php"), stems: &[], suffixes: &["php"] },
    Glyph { key: "prettier", asset: Some("prettier"), stems: &[], suffixes: &["prettier.config.cjs", "prettier.config.js", "prettier.config.mjs", "prettierignore", "prettierrc", "prettierrc.cjs", "prettierrc.js", "prettierrc.json", "prettierrc.json5", "prettierrc.mjs", "prettierrc.toml", "prettierrc.yaml", "prettierrc.yml"] },
    Glyph { key: "prisma", asset: Some("prisma"), stems: &[], suffixes: &["prisma"] },
    Glyph { key: "puppet", asset: Some("puppet"), stems: &[], suffixes: &["pp"] },
    Glyph { key: "python", asset: Some("python"), stems: &[], suffixes: &["py"] },
    Glyph { key: "r", asset: Some("r"), stems: &[], suffixes: &["r"] },
    Glyph { key: "react", asset: Some("react"), stems: &[], suffixes: &["cjsx", "ctsx", "jsx", "mjsx", "mtsx", "tsx"] },
    Glyph { key: "roc", asset: Some("roc"), stems: &[], suffixes: &["roc"] },
    Glyph { key: "ruby", asset: Some("ruby"), stems: &["podfile"], suffixes: &["rb"] },
    Glyph { key: "rust", asset: Some("rust"), stems: &[], suffixes: &["rs"] },
    Glyph { key: "sass", asset: Some("sass"), stems: &[], suffixes: &["sass", "scss"] },
    Glyph { key: "scala", asset: Some("scala"), stems: &[], suffixes: &["scala", "sc"] },
    Glyph { key: "settings", asset: Some("settings"), stems: &[], suffixes: &["conf", "ini"] },
    Glyph { key: "solidity", asset: Some("file"), stems: &[], suffixes: &["sol"] },
    Glyph { key: "storage", asset: Some("database"), stems: &[], suffixes: &["accdb", "csv", "dat", "db", "dbf", "dll", "fmp", "fp7", "frm", "gdb", "ib", "ldf", "mdb", "mdf", "myd", "myi", "pdb", "psv", "rdata", "sav", "sdf", "sql", "sqlite", "ssv", "tsv"] },
    Glyph { key: "stylelint", asset: Some("javascript"), stems: &[], suffixes: &["stylelint.config.cjs", "stylelint.config.js", "stylelint.config.mjs", "stylelintignore", "stylelintrc", "stylelintrc.cjs", "stylelintrc.js", "stylelintrc.json", "stylelintrc.mjs", "stylelintrc.yaml", "stylelintrc.yml"] },
    Glyph { key: "surrealql", asset: Some("surrealql"), stems: &[], suffixes: &["surql"] },
    Glyph { key: "svelte", asset: Some("html"), stems: &[], suffixes: &["svelte"] },
    Glyph { key: "swift", asset: Some("swift"), stems: &[], suffixes: &["swift"] },
    Glyph { key: "tcl", asset: Some("tcl"), stems: &[], suffixes: &["tcl"] },
    Glyph { key: "template", asset: Some("html"), stems: &[], suffixes: &["hbs", "plist", "xml"] },
    Glyph { key: "terminal", asset: Some("terminal"), stems: &[], suffixes: &["bash", "bash_aliases", "bash_login", "bash_logout", "bash_profile", "bashrc", "brushrc", "fish", "nu", "profile", "ps1", "sh", "zlogin", "zlogout", "zprofile", "zsh", "zsh_aliases", "zsh_histfile", "zsh_history", "zshenv", "zshrc"] },
    Glyph { key: "terraform", asset: Some("terraform"), stems: &[], suffixes: &["tf", "tfvars"] },
    Glyph { key: "toml", asset: Some("toml"), stems: &[], suffixes: &["toml"] },
    Glyph { key: "typescript", asset: Some("typescript"), stems: &[], suffixes: &["cts", "mts", "ts"] },
    Glyph { key: "v", asset: Some("v"), stems: &[], suffixes: &["v", "vsh", "vv"] },
    Glyph { key: "vbproj", asset: Some("file"), stems: &[], suffixes: &["vbproj"] },
    Glyph { key: "vcs", asset: Some("git"), stems: &[], suffixes: &["commit_editmsg", "edit_description", "merge_msg", "notes_editmsg", "tag_editmsg", "gitattributes", "gitignore", "gitkeep", "gitmodules"] },
    Glyph { key: "video", asset: Some("video"), stems: &[], suffixes: &["avi", "m4v", "mkv", "mov", "mp4", "webm", "wmv"] },
    Glyph { key: "vs_sln", asset: Some("file"), stems: &[], suffixes: &["sln"] },
    Glyph { key: "vs_suo", asset: Some("file"), stems: &[], suffixes: &["suo"] },
    Glyph { key: "vue", asset: Some("vue"), stems: &[], suffixes: &["vue"] },
    Glyph { key: "vyper", asset: Some("vyper"), stems: &[], suffixes: &["vy", "vyi"] },
    Glyph { key: "wgsl", asset: Some("wgsl"), stems: &[], suffixes: &["wgsl"] },
    Glyph { key: "yaml", asset: Some("yaml"), stems: &[], suffixes: &["yaml", "yml"] },
    Glyph { key: "zig", asset: Some("zig"), stems: &[], suffixes: &["zig"] },
];
