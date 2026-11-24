// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Pure file-type classification for the Editor's Source/Preview modes
//! (ADR 0015). No GPUI types: this decides which tabs have a preview, which
//! mode a file opens in, and whether the text buffer is involved at all.

use std::path::Path;

/// What kind of preview a file type has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewKind {
    /// Raster image (binary, never loaded as text).
    Image,
    /// SVG source; follows the text buffer.
    Svg,
    /// Markdown source; follows the text buffer.
    Markdown,
    /// Delimited table (csv/tsv/ssv/psv); follows the text buffer.
    Tabular,
    /// Binary PDF: no text form, the pane offers external open.
    Pdf,
    /// HTML source: the pane offers external open (no WebView).
    External,
}

/// Which content form an editor tab currently shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewMode {
    #[default]
    Source,
    Preview,
}

/// How a file opens: its initial mode.
pub type OpenMode = ViewMode;

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
}

/// The preview a file type has, if any. Plain text (`txt`) intentionally has
/// none: Source mode already is its best presentation.
pub fn classify(path: &Path) -> Option<PreviewKind> {
    Some(match extension(path)?.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" => PreviewKind::Image,
        "svg" => PreviewKind::Svg,
        "md" | "markdown" => PreviewKind::Markdown,
        "csv" | "tsv" | "ssv" | "psv" => PreviewKind::Tabular,
        "pdf" => PreviewKind::Pdf,
        "html" | "htm" => PreviewKind::External,
        _ => return None,
    })
}

impl PreviewKind {
    /// Binary kinds have no text form, so they open straight in Preview mode.
    pub fn is_binary(self) -> bool {
        matches!(self, PreviewKind::Image | PreviewKind::Pdf)
    }

    /// Kinds whose preview is derived from the editor's text buffer.
    pub fn follows_buffer(self) -> bool {
        matches!(
            self,
            PreviewKind::Svg | PreviewKind::Markdown | PreviewKind::Tabular
        )
    }

    /// Whether the toggle should be offered. Binary kinds have nothing to
    /// toggle to.
    pub fn has_source_form(self) -> bool {
        !self.is_binary()
    }
}

/// The mode `path` opens in.
pub fn open_mode(path: &Path) -> OpenMode {
    match classify(path) {
        Some(kind) if kind.is_binary() => ViewMode::Preview,
        _ => ViewMode::Source,
    }
}

/// Whether `path` is a file that only exists in Preview form (no text buffer).
pub fn is_preview_only(path: &Path) -> bool {
    open_mode(path) == ViewMode::Preview
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn classifies_known_types() {
        assert_eq!(classify(&p("a.PNG")), Some(PreviewKind::Image));
        assert_eq!(classify(&p("a.svg")), Some(PreviewKind::Svg));
        assert_eq!(classify(&p("a.md")), Some(PreviewKind::Markdown));
        assert_eq!(classify(&p("a.tsv")), Some(PreviewKind::Tabular));
        assert_eq!(classify(&p("a.pdf")), Some(PreviewKind::Pdf));
        assert_eq!(classify(&p("a.txt")), None);
        assert_eq!(classify(&p("Makefile")), None);
    }

    #[test]
    fn binary_files_open_in_preview_text_files_in_source() {
        assert_eq!(open_mode(&p("a.jpg")), ViewMode::Preview);
        assert_eq!(open_mode(&p("a.pdf")), ViewMode::Preview);
        assert_eq!(open_mode(&p("a.svg")), ViewMode::Source);
        assert_eq!(open_mode(&p("a.md")), ViewMode::Source);
        assert_eq!(open_mode(&p("a.html")), ViewMode::Source);
        assert_eq!(open_mode(&p("a.rs")), ViewMode::Source);
    }

    #[test]
    fn toggle_only_for_kinds_with_a_text_form() {
        assert!(!PreviewKind::Image.has_source_form());
        assert!(PreviewKind::Svg.has_source_form());
        assert!(PreviewKind::Markdown.follows_buffer());
        assert!(!PreviewKind::Pdf.follows_buffer());
        assert!(PreviewKind::External.has_source_form());
    }
}
