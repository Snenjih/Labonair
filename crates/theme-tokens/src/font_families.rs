// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! The bundled font-family names + platform fallbacks. The embedded font bytes
//! and system-font discovery stay in `labonair-theme::fonts`; only these
//! string constants are needed by the token defaults, so they live here.

/// Bundled UI-chrome font family (Inter Variable).
pub const UI_FONT_FAMILY: &str = "Inter Variable";

/// Bundled editor/terminal monospace font family (JetBrains Mono).
pub const MONO_FONT_FAMILY: &str = "JetBrains Mono";

/// Platform fallbacks appended after [`UI_FONT_FAMILY`].
pub const UI_FONT_FALLBACKS: &[&str] = &[".SystemUIFont", "sans-serif"];

/// Platform fallbacks appended after [`MONO_FONT_FAMILY`].
pub const MONO_FONT_FALLBACKS: &[&str] = &["SFMono-Regular", "Menlo", "monospace"];
