// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Compatibility re-export; the preview renderer is owned by
//! `labonair-editor-ui` (ADR 0015). Removed together with the legacy Preview
//! pane.

pub use labonair_editor_ui::preview::*;
pub use labonair_editor_ui::PreviewView;
