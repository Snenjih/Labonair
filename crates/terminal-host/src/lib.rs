// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Detached session host (ADR 0009): wire [`protocol`] and the daemon
//! [`server`]. GPUI-free; the daemon hosts the SSH stack through [`stack`].

#[cfg(unix)]
pub mod client;
pub mod protocol;
#[cfg(unix)]
pub mod proxy;
#[cfg(unix)]
pub mod server;
#[cfg(unix)]
pub mod spawn;
#[cfg(unix)]
pub mod stack;
