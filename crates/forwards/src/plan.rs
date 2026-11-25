// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Pure helpers over a host's saved forwards: the saved-vs-running diff,
//! cross-host listen-port conflicts and palette row merging (ADR 0016 §3).

use labonair_ssh::{
    ForwardId, ForwardInfo, ForwardKind, ForwardLifetime, ForwardOrigin, ForwardState,
};

use crate::palette::ForwardRow;
use crate::saved::SavedForward;

/// One step to bring running forwards in line with the saved list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Saved auto-start forward that is not running.
    Open(SavedForward),
    /// Running saved forward that no longer exists in the saved list.
    Close(ForwardId),
    /// Running forward whose saved endpoints changed: close, then open the new spec.
    Restart(SavedForward),
}

fn same_endpoints(saved: &SavedForward, info: &ForwardInfo) -> bool {
    let s = &info.spec;
    saved.kind == s.kind
        && saved.listen_host == s.listen_host
        && saved.listen_port == s.listen_port
        && saved.target_host == s.target_host
        && saved.target_port == s.target_port
}

/// Diffs the saved forwards of one host against its live forwards (matched by
/// id). Ad-hoc forwards (`saved == false`) are never closed. `Open` is only
/// meaningful for a host that is currently connected; callers apply the plan
/// only when the host has running forwards.
pub fn plan_forward_changes(saved: &[SavedForward], running: &[ForwardInfo]) -> Vec<Change> {
    let live = |id: &str| running.iter().find(|i| i.spec.id.0 == id);
    let mut out = Vec::new();
    for info in running.iter().filter(|i| i.saved && i.is_running()) {
        if !saved.iter().any(|s| s.id == info.spec.id.0) {
            out.push(Change::Close(info.spec.id.clone()));
        }
    }
    for s in saved {
        match live(&s.id).filter(|i| i.is_running()) {
            Some(info) if !same_endpoints(s, info) => out.push(Change::Restart(s.clone())),
            Some(_) => {}
            None if s.auto_start => out.push(Change::Open(s.clone())),
            None => {}
        }
    }
    out
}

/// A host's saved forwards with its display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostForwards {
    pub host_id: String,
    pub host_name: String,
    pub forwards: Vec<SavedForward>,
}

fn binds_locally(kind: ForwardKind) -> bool {
    kind != ForwardKind::Remote
}

/// Human-readable hints for other hosts' saved forwards that listen on the
/// same `listen_host:listen_port` as `spec`. Port 0 (any free port) and
/// remote forwards (they listen on the server) never conflict.
pub fn listen_conflicts(all: &[HostForwards], host_id: &str, spec: &SavedForward) -> Vec<String> {
    if spec.listen_port == 0 || !binds_locally(spec.kind) {
        return Vec::new();
    }
    all.iter()
        .filter(|h| h.host_id != host_id)
        .flat_map(|h| h.forwards.iter().map(move |f| (h, f)))
        .filter(|(_, f)| {
            binds_locally(f.kind)
                && f.listen_port == spec.listen_port
                && f.listen_host.eq_ignore_ascii_case(&spec.listen_host)
        })
        .map(|(h, f)| {
            format!(
                "{}:{} is also used by a saved forward of {}",
                f.listen_host, f.listen_port, h.host_name
            )
        })
        .collect()
}

/// Palette rows for the saved forwards of every host (stopped unless the
/// service knows them as running) plus live forwards that are not saved.
pub fn forward_rows(
    hosts: &[HostForwards],
    live: Vec<ForwardInfo>,
    host_name: impl Fn(&str) -> String,
) -> Vec<ForwardRow> {
    let live_ref = &live;
    let mut rows: Vec<ForwardRow> = hosts
        .iter()
        .flat_map(|h| {
            h.forwards.iter().map(move |f| {
                let info = live_ref
                    .iter()
                    .find(|i| i.spec.id.0 == f.id)
                    .cloned()
                    .map(|mut i| {
                        i.saved = true;
                        i
                    })
                    .unwrap_or_else(|| ForwardInfo {
                        spec: f.clone().into_spec(&h.host_id),
                        state: ForwardState::Stopped,
                        lifetime: ForwardLifetime::Pinned,
                        origin: ForwardOrigin::App,
                        bound_port: None,
                        connections: 0,
                        bytes_in: 0,
                        bytes_out: 0,
                        saved: true,
                    });
                ForwardRow {
                    info,
                    host_name: h.host_name.clone(),
                }
            })
        })
        .collect();
    for info in live {
        if !rows.iter().any(|r| r.info.spec.id == info.spec.id) {
            rows.push(ForwardRow {
                host_name: host_name(&info.spec.host_id),
                info,
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(id: &str, port: u16, auto: bool) -> SavedForward {
        SavedForward {
            id: id.into(),
            kind: ForwardKind::Local,
            name: None,
            listen_host: "127.0.0.1".into(),
            listen_port: port,
            target_host: "db".into(),
            target_port: 5432,
            auto_start: auto,
        }
    }

    fn info(s: &SavedForward, state: ForwardState, is_saved: bool) -> ForwardInfo {
        ForwardInfo {
            spec: s.clone().into_spec("h"),
            state,
            lifetime: ForwardLifetime::WithConnection,
            origin: ForwardOrigin::App,
            bound_port: None,
            connections: 0,
            bytes_in: 0,
            bytes_out: 0,
            saved: is_saved,
        }
    }

    #[test]
    fn plan_open_close_restart() {
        let a = saved("a", 8080, true);
        let mut a2 = a.clone();
        a2.target_port = 1;
        let b = saved("b", 9000, true);
        let c = saved("c", 9001, false);
        let running = vec![
            info(&a, ForwardState::Active, true),
            info(&b, ForwardState::Active, true),
            info(&saved("adhoc", 1, true), ForwardState::Active, false),
        ];
        // a changed, b removed, c is not auto-start, d is new auto-start.
        let d = saved("d", 7000, true);
        let plan = plan_forward_changes(&[a2.clone(), c, d.clone()], &running);
        assert_eq!(
            plan,
            vec![
                Change::Close(ForwardId("b".into())),
                Change::Restart(a2),
                Change::Open(d)
            ]
        );
        assert!(plan_forward_changes(
            std::slice::from_ref(&a),
            &[info(&a, ForwardState::Active, true)]
        )
        .is_empty());
        // A stopped forward counts as not running.
        assert_eq!(
            plan_forward_changes(
                std::slice::from_ref(&a),
                &[info(&a, ForwardState::Stopped, true)]
            ),
            vec![Change::Open(a)]
        );
    }

    #[test]
    fn conflicts_other_hosts_only() {
        let all = vec![
            HostForwards {
                host_id: "h1".into(),
                host_name: "one".into(),
                forwards: vec![saved("a", 8080, true), saved("z", 0, true)],
            },
            HostForwards {
                host_id: "h2".into(),
                host_name: "two".into(),
                forwards: vec![saved("b", 8080, true), saved("y", 0, true)],
            },
        ];
        let hit = listen_conflicts(&all, "h1", &saved("new", 8080, true));
        assert_eq!(hit.len(), 1);
        assert!(hit.iter().any(|m| m.contains("two")));
        assert_eq!(
            listen_conflicts(&all, "h2", &saved("new", 8080, true)).len(),
            1
        );
        assert!(listen_conflicts(&all, "h1", &saved("new", 0, true)).is_empty());
        let mut other_bind = saved("new", 8080, true);
        other_bind.listen_host = "0.0.0.0".into();
        assert!(listen_conflicts(&all, "h1", &other_bind).is_empty());
        let mut remote = saved("new", 8080, true);
        remote.kind = ForwardKind::Remote;
        assert!(listen_conflicts(&all, "h1", &remote).is_empty());
    }

    #[test]
    fn rows_cover_all_hosts_and_live() {
        let a = saved("a", 8080, true);
        let b = saved("b", 9000, true);
        let hosts = vec![
            HostForwards {
                host_id: "h".into(),
                host_name: "H".into(),
                forwards: vec![a.clone()],
            },
            HostForwards {
                host_id: "g".into(),
                host_name: "G".into(),
                forwards: vec![b],
            },
        ];
        let adhoc = info(&saved("x", 1, true), ForwardState::Active, false);
        let rows = forward_rows(
            &hosts,
            vec![info(&a, ForwardState::Active, false), adhoc],
            |id| format!("name-{id}"),
        );
        assert_eq!(rows.len(), 3);
        assert!(rows[0].info.is_running() && rows[0].info.saved);
        assert!(!rows[1].info.is_running() && rows[1].info.saved);
        assert_eq!(rows[1].info.spec.host_id, "g");
        assert!(!rows[2].info.saved);
        assert_eq!(rows[2].host_name, "name-h");
    }
}
