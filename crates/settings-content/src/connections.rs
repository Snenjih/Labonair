// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! `connections` area — global SSH connection defaults. Per-host values
//! (`hosts.keep_alive_interval`, `hosts.keep_alive_tries`) still win when a
//! host record sets them explicitly; these are only the fallback applied to
//! hosts that leave them unset, plus the one value (connect timeout) that has
//! no host-level override today.

use crate::MergeFrom;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema, MergeFrom,
)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnectionsContent {
    /// Seconds to wait for the initial TCP+SSH handshake before giving up.
    pub ssh_connect_timeout_secs: Option<u64>,
    /// Fallback keep-alive ping interval, in seconds, for hosts that don't
    /// set their own.
    pub ssh_keepalive_interval_secs: Option<u64>,
    /// Fallback number of missed keep-alive pings tolerated before the
    /// connection is considered dead, for hosts that don't set their own.
    pub ssh_keepalive_max_failures: Option<u32>,
    /// Default listen address of new port forwards (`forwards.defaultListenHost`).
    pub forwards_default_listen_host: Option<String>,
    /// Reconnect dropped port forwards automatically (`forwards.autoReconnect`).
    pub forwards_auto_reconnect: Option<bool>,
    /// Ask before binding a non-loopback address (`forwards.confirmPublicBind`).
    pub forwards_confirm_public_bind: Option<bool>,
}

impl ConnectionsContent {
    pub fn defaults() -> Self {
        Self {
            ssh_connect_timeout_secs: Some(20),
            ssh_keepalive_interval_secs: Some(25),
            ssh_keepalive_max_failures: Some(3),
            forwards_default_listen_host: Some("127.0.0.1".into()),
            forwards_auto_reconnect: Some(true),
            forwards_confirm_public_bind: Some(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_defaults_and_round_trip() {
        let d = ConnectionsContent::defaults();
        assert_eq!(d.forwards_default_listen_host.as_deref(), Some("127.0.0.1"));
        assert_eq!(d.forwards_auto_reconnect, Some(true));
        assert_eq!(d.forwards_confirm_public_bind, Some(true));
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["forwardsAutoReconnect"], true);
        assert_eq!(json["forwardsDefaultListenHost"], "127.0.0.1");
        let back: ConnectionsContent = serde_json::from_value(json).unwrap();
        assert_eq!(back, d);
    }
}
