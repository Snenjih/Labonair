// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! GPUI-free port-forward helpers: saved-forward codec, validation, quick
//! (`-L`/`-R`/`-D`) parsing and display labels. Governed by ADR 0016.

pub mod draft;
pub mod label;
pub mod palette;
pub mod plan;
pub mod quick;
pub mod saved;
pub mod ssh_config;
pub mod validate;

pub use draft::{
    plan_submit, DraftField, EditorDefaults, FieldError, ForwardDraft, ForwardEditorEvent,
    SubmitAction,
};
pub use label::{kind_glyph, kind_name, listen_label, route_label};
pub use palette::{
    active_forwards_submenu, decode_action, dynamic_rows, forwards_hosts_submenu,
    route_hosts_submenu, saved_forwards_submenu, via_targets_submenu, ForwardRow,
    ForwardsCommandProvider, ForwardsIntent, ForwardsUiEvent, HostChoice,
};
pub use plan::{forward_rows, listen_conflicts, plan_forward_changes, Change, HostForwards};
pub use quick::{parse_quick, QuickSpec};
pub use saved::{parse_saved, serialize_saved, SavedForward};
pub use ssh_config::{from_ssh_config_line, to_ssh_config_line};
pub use validate::{is_public_bind, validate_spec, ValidationError};
