// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Pure model behind the forward editor: string fields, validation, spec
//! conversion, public-bind warning and the live diagram (ADR 0016 §4).

use std::sync::atomic::{AtomicU64, Ordering};

use labonair_ssh::{ForwardId, ForwardKind, ForwardLifetime, ForwardOpenRequest, ForwardSpec};

use crate::quick::QuickSpec;
use crate::saved::SavedForward;
use crate::validate::is_public_bind;

/// Editor defaults; the settings (`forwards.*`, U5) feed these later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorDefaults {
    pub listen_host: String,
    pub confirm_public_bind: bool,
}

impl EditorDefaults {
    /// Maps the `forwards.*` settings; a blank listen host falls back to loopback.
    pub fn from_settings(listen_host: String, confirm_public_bind: bool) -> Self {
        let host = listen_host.trim();
        Self {
            listen_host: if host.is_empty() {
                "127.0.0.1".into()
            } else {
                host.into()
            },
            confirm_public_bind,
        }
    }
}

impl Default for EditorDefaults {
    fn default() -> Self {
        Self {
            listen_host: "127.0.0.1".into(),
            confirm_public_bind: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftField {
    ListenHost,
    ListenPort,
    TargetHost,
    TargetPort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: DraftField,
    pub message: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardDraft {
    id: String,
    pub kind: ForwardKind,
    pub name: String,
    pub listen_host: String,
    pub listen_port: String,
    pub target_host: String,
    pub target_port: String,
    pub auto_start: bool,
    pub save_to_host: bool,
}

fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("fwd-{nanos:x}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

fn port_error(raw: &str, allow_zero: bool) -> Option<&'static str> {
    match raw.trim().parse::<u16>() {
        Ok(0) if !allow_zero => Some("Port must be between 1 and 65535"),
        Ok(_) => None,
        Err(_) if raw.trim().is_empty() => Some("Enter a port"),
        Err(_) => Some("Port must be a number from 0 to 65535"),
    }
}

impl ForwardDraft {
    pub fn new(defaults: &EditorDefaults) -> Self {
        Self {
            id: new_id(),
            kind: ForwardKind::Local,
            name: String::new(),
            listen_host: defaults.listen_host.clone(),
            listen_port: String::new(),
            target_host: String::new(),
            target_port: String::new(),
            auto_start: true,
            save_to_host: false,
        }
    }

    pub fn from_quick(q: &QuickSpec, defaults: &EditorDefaults) -> Self {
        Self {
            kind: q.kind,
            listen_host: q.listen_host.clone(),
            listen_port: q.listen_port.to_string(),
            target_host: q.target_host.clone(),
            target_port: if q.target_port == 0 {
                String::new()
            } else {
                q.target_port.to_string()
            },
            ..Self::new(defaults)
        }
    }

    pub fn from_saved(s: &SavedForward) -> Self {
        Self {
            id: s.id.clone(),
            kind: s.kind,
            name: s.name.clone().unwrap_or_default(),
            listen_host: s.listen_host.clone(),
            listen_port: s.listen_port.to_string(),
            target_host: s.target_host.clone(),
            target_port: if s.target_port == 0 {
                String::new()
            } else {
                s.target_port.to_string()
            },
            auto_start: s.auto_start,
            save_to_host: true,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn validate(&self) -> Vec<FieldError> {
        let mut errors = Vec::new();
        let mut push = |field, message| errors.push(FieldError { field, message });
        if self.listen_host.trim().is_empty() {
            push(DraftField::ListenHost, "Listen address must not be empty");
        }
        // Listen port 0 picks a free port.
        if let Some(m) = port_error(&self.listen_port, true) {
            push(DraftField::ListenPort, m);
        }
        if self.kind != ForwardKind::Dynamic {
            if self.target_host.trim().is_empty() {
                push(DraftField::TargetHost, "Target host must not be empty");
            }
            if let Some(m) = port_error(&self.target_port, false) {
                push(DraftField::TargetPort, m);
            }
        }
        errors
    }

    pub fn to_spec(&self, host_id: &str) -> Result<ForwardSpec, Vec<FieldError>> {
        let errors = self.validate();
        if !errors.is_empty() {
            return Err(errors);
        }
        let dynamic = self.kind == ForwardKind::Dynamic;
        let port = |s: &str| s.trim().parse::<u16>().unwrap_or_default();
        Ok(ForwardSpec {
            id: ForwardId(self.id.clone()),
            host_id: host_id.to_string(),
            kind: self.kind,
            name: Some(self.name.trim().to_string()).filter(|n| !n.is_empty()),
            listen_host: self.listen_host.trim().to_string(),
            listen_port: port(&self.listen_port),
            target_host: if dynamic {
                String::new()
            } else {
                self.target_host.trim().to_string()
            },
            target_port: if dynamic { 0 } else { port(&self.target_port) },
        })
    }

    /// The `hosts.tunnels` entry for this draft.
    pub fn to_saved(&self, host_id: &str) -> Result<SavedForward, Vec<FieldError>> {
        let s = self.to_spec(host_id)?;
        Ok(SavedForward {
            id: s.id.0,
            kind: s.kind,
            name: s.name,
            listen_host: s.listen_host,
            listen_port: s.listen_port,
            target_host: s.target_host,
            target_port: s.target_port,
            auto_start: self.auto_start,
        })
    }

    pub fn public_bind_warning(&self) -> Option<&'static str> {
        let host = self.listen_host.trim();
        (!host.is_empty() && is_public_bind(host)).then_some(
            "Binds beyond loopback: anyone who can reach this machine's network can use this forward.",
        )
    }

    pub fn diagram(&self) -> String {
        let or = |s: &str, fallback: &str| {
            let s = s.trim();
            if s.is_empty() { fallback } else { s }.to_string()
        };
        let listen = format!(
            "{}:{}",
            or(&self.listen_host, "?"),
            or(&self.listen_port, "?")
        );
        let target = format!(
            "{}:{}",
            or(&self.target_host, "?"),
            or(&self.target_port, "?")
        );
        match self.kind {
            ForwardKind::Local => format!("{listen} ──ssh──▶ {target}"),
            ForwardKind::Remote => {
                format!("server:{} ◀──ssh── {target}", or(&self.listen_port, "?"))
            }
            ForwardKind::Dynamic => format!("SOCKS5 {listen} ──ssh──▶ any"),
        }
    }
}

/// What the editor asks the workspace to do.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)] // one-shot UI event, never stored in bulk
pub enum ForwardEditorEvent {
    Submit {
        spec: ForwardSpec,
        lifetime: ForwardLifetime,
        /// Persist into the host's saved forwards.
        save_to_host: Option<SavedForward>,
        /// False for `Save only`.
        start: bool,
    },
    Cancel,
}

/// One step the workspace performs for a submit, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitAction {
    Save {
        host_id: String,
        forward: SavedForward,
    },
    Open(ForwardOpenRequest),
}

pub fn plan_submit(event: ForwardEditorEvent) -> Vec<SubmitAction> {
    let ForwardEditorEvent::Submit {
        spec,
        lifetime,
        save_to_host,
        start,
    } = event
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(forward) = save_to_host {
        out.push(SubmitAction::Save {
            host_id: spec.host_id.clone(),
            forward,
        });
    }
    if start {
        out.push(SubmitAction::Open(ForwardOpenRequest { spec, lifetime }));
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn editor_defaults_map_settings() {
        let d = EditorDefaults::from_settings(" 0.0.0.0 ".into(), false);
        assert_eq!(
            (d.listen_host.as_str(), d.confirm_public_bind),
            ("0.0.0.0", false)
        );
        assert_eq!(
            EditorDefaults::from_settings("".into(), true),
            EditorDefaults::default()
        );
        assert_eq!(ForwardDraft::new(&d).listen_host, "0.0.0.0");
    }

    use super::*;

    fn draft(kind: ForwardKind, lp: &str, th: &str, tp: &str) -> ForwardDraft {
        ForwardDraft {
            kind,
            listen_port: lp.into(),
            target_host: th.into(),
            target_port: tp.into(),
            ..ForwardDraft::new(&EditorDefaults::default())
        }
    }

    #[test]
    fn all_three_kinds_validate_and_convert() {
        let l = draft(ForwardKind::Local, "8080", "db", "5432");
        assert!(l.validate().is_empty());
        let s = l.to_spec("h").unwrap();
        assert_eq!(
            (s.listen_port, s.target_port, s.host_id.as_str()),
            (8080, 5432, "h")
        );
        let r = draft(ForwardKind::Remote, "9000", "localhost", "3000");
        assert_eq!(r.to_spec("h").unwrap().target_host, "localhost");
        let mut d = draft(ForwardKind::Dynamic, "1080", "ignored", "");
        d.name = " socks ".into();
        let s = d.to_spec("h").unwrap();
        assert_eq!((s.target_host.as_str(), s.target_port), ("", 0));
        assert_eq!(s.name.as_deref(), Some("socks"));
    }

    #[test]
    fn port_zero_and_invalid_ports() {
        assert_eq!(
            draft(ForwardKind::Local, "0", "db", "1")
                .to_spec("h")
                .unwrap()
                .listen_port,
            0
        );
        let fields = |d: ForwardDraft| d.validate().iter().map(|e| e.field).collect::<Vec<_>>();
        assert_eq!(
            fields(draft(ForwardKind::Local, "", "db", "1")),
            [DraftField::ListenPort]
        );
        assert_eq!(
            fields(draft(ForwardKind::Local, "70000", "db", "1")),
            [DraftField::ListenPort]
        );
        assert_eq!(
            fields(draft(ForwardKind::Local, "1", "db", "0")),
            [DraftField::TargetPort]
        );
        assert_eq!(
            fields(draft(ForwardKind::Local, "1", "db", "x")),
            [DraftField::TargetPort]
        );
        assert!(draft(ForwardKind::Local, "70000", "db", "1")
            .to_spec("h")
            .is_err());
    }

    #[test]
    fn empty_hosts() {
        let mut d = draft(ForwardKind::Local, "1", " ", "1");
        d.listen_host = " ".into();
        let fields: Vec<_> = d.validate().iter().map(|e| e.field).collect();
        assert_eq!(fields, [DraftField::ListenHost, DraftField::TargetHost]);
        d.kind = ForwardKind::Dynamic;
        assert_eq!(d.validate().len(), 1);
    }

    #[test]
    fn public_bind() {
        let mut d = draft(ForwardKind::Local, "1", "db", "1");
        assert!(d.public_bind_warning().is_none());
        for h in ["0.0.0.0", "192.168.1.2", "::"] {
            d.listen_host = h.into();
            assert!(d.public_bind_warning().is_some(), "{h}");
        }
        d.listen_host = "localhost".into();
        assert!(d.public_bind_warning().is_none());
        d.listen_host = " ".into();
        assert!(d.public_bind_warning().is_none());
    }

    #[test]
    fn diagrams() {
        let mut d = draft(ForwardKind::Local, "8080", "db", "5432");
        d.listen_host = "localhost".into();
        assert_eq!(d.diagram(), "localhost:8080 ──ssh──▶ db:5432");
        d.kind = ForwardKind::Remote;
        d.listen_port = "9000".into();
        d.target_host = "localhost".into();
        d.target_port = "3000".into();
        assert_eq!(d.diagram(), "server:9000 ◀──ssh── localhost:3000");
        d.kind = ForwardKind::Dynamic;
        d.listen_host = "127.0.0.1".into();
        d.listen_port = "1080".into();
        assert_eq!(d.diagram(), "SOCKS5 127.0.0.1:1080 ──ssh──▶ any");
        d.listen_port.clear();
        assert_eq!(d.diagram(), "SOCKS5 127.0.0.1:? ──ssh──▶ any");
    }

    #[test]
    fn saved_and_quick_round_trip() {
        let mut d = draft(ForwardKind::Local, "8080", "db", "5432");
        d.auto_start = false;
        let saved = d.to_saved("h").unwrap();
        assert!(!saved.auto_start);
        let back = ForwardDraft::from_saved(&saved);
        assert_eq!(back.id(), d.id());
        assert_eq!(back.to_saved("h").unwrap(), saved);
        let q = crate::parse_quick("-D 0.0.0.0:1080").unwrap();
        let qd = ForwardDraft::from_quick(&q, &EditorDefaults::default());
        assert_eq!(
            (qd.kind, qd.listen_host.as_str(), qd.target_port.as_str()),
            (ForwardKind::Dynamic, "0.0.0.0", "")
        );
    }

    #[test]
    fn submit_mapping() {
        let spec = draft(ForwardKind::Local, "1", "db", "1")
            .to_spec("h")
            .unwrap();
        let saved = draft(ForwardKind::Local, "1", "db", "1")
            .to_saved("h")
            .unwrap();
        let ev = |lifetime, save: Option<SavedForward>, start| ForwardEditorEvent::Submit {
            spec: spec.clone(),
            lifetime,
            save_to_host: save,
            start,
        };
        let open = |lifetime| {
            SubmitAction::Open(ForwardOpenRequest {
                spec: spec.clone(),
                lifetime,
            })
        };
        assert_eq!(
            plan_submit(ev(ForwardLifetime::Pinned, None, true)),
            [open(ForwardLifetime::Pinned)]
        );
        assert_eq!(
            plan_submit(ev(
                ForwardLifetime::WithConnection,
                Some(saved.clone()),
                true
            )),
            [
                SubmitAction::Save {
                    host_id: "h".into(),
                    forward: saved.clone()
                },
                open(ForwardLifetime::WithConnection)
            ]
        );
        assert_eq!(
            plan_submit(ev(
                ForwardLifetime::WithConnection,
                Some(saved.clone()),
                false
            )),
            [SubmitAction::Save {
                host_id: "h".into(),
                forward: saved
            }]
        );
        assert!(plan_submit(ForwardEditorEvent::Cancel).is_empty());
    }
}
