// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! GPUI presentation of port forwards (ADR 0016 §4): the `forwards` statusbar
//! item and its popover. The view never talks to services; it emits typed
//! [`ForwardsUiEvent`]s that the composition root performs.

mod editor;
mod prompt;
pub(crate) mod rows;
mod status_item;
mod view;

pub use editor::ForwardEditor;
pub use prompt::{ForwardPromptDialog, ForwardPromptEvent};
pub use rows::{
    aggregate_health, build_rows, group_by_host, looks_like_http, primary_action, state_text,
    ForwardRow, Health, HostGroup,
};
pub use status_item::{status_item_registration, ForwardsStatusItem};
pub use view::ForwardsView;
