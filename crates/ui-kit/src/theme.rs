// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Theme-token access for the ui-kit primitives.
//!
//! The [`UiTheme`] / [`ActiveThemeExt`] contracts moved to the foundation-level
//! `labonair-theme-tokens` crate in R08-008 (so ui-kit no longer depends on the
//! Themes *feature* crate). `labonair-theme` implements `UiTheme` for its
//! runtime `ThemeStore`. This module just re-exports the contracts so existing
//! `labonair_ui_kit::{UiTheme, ActiveThemeExt}` call sites keep working.

pub use labonair_theme_tokens::{ActiveThemeExt, UiTheme};
