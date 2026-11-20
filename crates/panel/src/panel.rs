// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! `labonair-panel` — the **contracts** shared by the workspace, the panel
//! feature crates and the shell.
//!
//! This crate exists to break the dependency cycle "panels need workspace
//! types, the workspace needs the panel trait". It holds only trait/enum
//! declarations and two small registry containers — no rendering, no
//! persistence, no feature code. Per `docs/architecture.md` §3 it must depend
//! on **no** workspace-track crate (`labonair-workspace`, `labonair-shell`) and
//! on **no** `labonair-panel-*` crate; its only dependencies are `gpui` and
//! `labonair-gpui-ext`.
//!
//! The contracts are reduced to what Labonair's fixed four-zone layout needs.
//! They remain intentionally **unused** until the workspace and status bar
//! integrations in T17-001/003 are complete.

mod dock;
mod project_diff;
mod status;

pub use dock::{
    AnyPanelHandle, DockPosition, Panel, PanelConstructor, PanelEvent, PanelHandle, PanelIcon,
    PanelRegistration, PanelRegistry,
};
pub use project_diff::{DiffSource, ProjectDiffFile, ProjectDiffMode, ProjectDiffRequest};
pub use status::{
    AnyStatusItemHandle, StatusItem, StatusItemConstructor, StatusItemHandle, StatusItemHide,
    StatusItemRegistration, StatusItemRegistry, StatusMenuEntry, StatusPlacement, StatusSide,
};
