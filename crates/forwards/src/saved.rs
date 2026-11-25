// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! `hosts.tunnels` JSON column codec. Legacy keys are read via aliases.

use labonair_ssh::{ForwardId, ForwardKind, ForwardSpec};
use serde::{Deserialize, Serialize};

fn default_listen_host() -> String {
    "127.0.0.1".into()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedForward {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: ForwardKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default = "default_listen_host")]
    pub listen_host: String,
    #[serde(alias = "local_port")]
    pub listen_port: u16,
    #[serde(alias = "remote_host", default)]
    pub target_host: String,
    #[serde(alias = "remote_port", default)]
    pub target_port: u16,
    #[serde(default = "default_true")]
    pub auto_start: bool,
}

impl SavedForward {
    pub fn into_spec(self, host_id: &str) -> ForwardSpec {
        ForwardSpec {
            id: ForwardId(self.id),
            host_id: host_id.to_string(),
            kind: self.kind,
            name: self.name,
            listen_host: self.listen_host,
            listen_port: self.listen_port,
            target_host: self.target_host,
            target_port: self.target_port,
        }
    }
}

/// Parses the raw column; empty or malformed input yields an empty list.
pub fn parse_saved(raw: &Option<String>) -> Vec<SavedForward> {
    raw.as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

pub fn serialize_saved(forwards: &[SavedForward]) -> String {
    serde_json::to_string(forwards).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_json_round_trips_with_new_keys() {
        let raw = Some(
            r#"[{"id":"a","type":"local","local_port":8080,"remote_host":"db","remote_port":5432}]"#
                .to_string(),
        );
        let v = parse_saved(&raw);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, ForwardKind::Local);
        assert_eq!(v[0].listen_host, "127.0.0.1");
        assert_eq!(v[0].listen_port, 8080);
        assert_eq!(v[0].target_host, "db");
        assert_eq!(v[0].target_port, 5432);
        assert!(v[0].auto_start);
        let out = serialize_saved(&v);
        assert!(out.contains("listen_port") && out.contains("target_host"));
        assert!(!out.contains("local_port") && !out.contains("remote_host"));
        assert_eq!(parse_saved(&Some(out)), v);
    }

    #[test]
    fn garbage_and_empty_are_empty() {
        assert!(parse_saved(&None).is_empty());
        assert!(parse_saved(&Some(String::new())).is_empty());
        assert!(parse_saved(&Some("nope".into())).is_empty());
    }

    #[test]
    fn dynamic_and_defaults_and_spec() {
        let v = parse_saved(&Some(
            r#"[{"id":"d","type":"dynamic","listen_port":1080,"auto_start":false}]"#.into(),
        ));
        assert_eq!(v[0].kind, ForwardKind::Dynamic);
        assert!(!v[0].auto_start);
        let spec = v[0].clone().into_spec("h1");
        assert_eq!(spec.host_id, "h1");
        assert_eq!(spec.id, ForwardId("d".into()));
        assert_eq!(spec.target_port, 0);
    }
}
