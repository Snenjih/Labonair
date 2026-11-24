// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! GPUI presentation and interaction for the Editor capability.
//!
//! Workspace creates and hosts [`EditorView`] instances, while the editor
//! owner keeps text behavior, rendering, settings application, and commands
//! in this sibling UI crate.

mod editor;
pub mod markdown;
mod multibuffer;
pub mod preview;
pub mod preview_mode;
mod status_item;
mod syntax_theme;

pub use editor::{EditorBufferSnapshot, EditorEvent, EditorView};
pub use multibuffer::{MultiBufferEvent, MultiBufferView};
pub use preview::{PreviewImageAction, PreviewView};
pub use preview_mode::{OpenMode, PreviewKind, ViewMode};
pub use status_item::{status_item_registration, ActiveEditorProvider};
pub use syntax_theme::EditorPalette;
