// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Workspace-hosted views that remain part of Workspace orchestration.
//!
//! Moved out of `crates/ui` in T16-006. `git_graph` moved to
//! `labonair-panel-git-graph`; `hosts` reaches the Hosts UI through the narrow
//! `labonair-hosts-host::HostView` contract (R08-012); the connection-status
//! store is `crate::ssh_connection`. The editor module is only a temporary
//! re-export; its implementation belongs to `labonair-editor-ui`.

pub mod editor;
pub mod project_diff;
pub mod sftp;
pub mod terminal;
