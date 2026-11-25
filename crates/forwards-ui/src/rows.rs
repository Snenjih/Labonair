// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Pure row model for the forwards popover. GPUI-free.

use labonair_forwards::ForwardsUiEvent;
use labonair_ssh::{ForwardInfo, ForwardKind, ForwardState};

/// A forward plus the host label and jump route it is shown under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardRow {
    pub info: ForwardInfo,
    pub host_label: String,
    /// Jump hosts, outermost first (empty = direct).
    pub route: Vec<String>,
}

/// Aggregate state shown by the statusbar dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Active,
    /// Starting or reconnecting (amber).
    Busy,
    /// Waiting for a prompt answer (blue).
    NeedsInput,
    Failed,
}

/// Worst state wins: failed > needs input > busy > active.
pub fn aggregate_health(forwards: &[ForwardInfo]) -> Health {
    let mut health = Health::Active;
    for info in forwards {
        let h = match info.state {
            ForwardState::Failed { .. } => Health::Failed,
            ForwardState::AwaitingInput(_) => Health::NeedsInput,
            ForwardState::Starting | ForwardState::Reconnecting { .. } => Health::Busy,
            ForwardState::Active | ForwardState::Stopped => Health::Active,
        };
        let rank = |h: Health| match h {
            Health::Active => 0,
            Health::Busy => 1,
            Health::NeedsInput => 2,
            Health::Failed => 3,
        };
        if rank(h) > rank(health) {
            health = h;
        }
    }
    health
}

/// Ports whose targets are usually HTTP(S) services.
pub fn looks_like_http(port: u16) -> bool {
    matches!(
        port,
        80 | 443 | 3000 | 3001 | 4200 | 5000 | 5173 | 8000 | 8080 | 8443 | 8888 | 9000
    )
}

pub fn state_text(state: &ForwardState) -> String {
    match state {
        ForwardState::Starting => "Starting".into(),
        ForwardState::Active => "Active".into(),
        ForwardState::Reconnecting { attempt } => format!("Reconnecting ({attempt})"),
        ForwardState::AwaitingInput(_) => "Needs input".into(),
        ForwardState::Failed { reason } => format!("Failed: {reason}"),
        ForwardState::Stopped => "Stopped".into(),
    }
}

/// Build rows; `host` resolves `host_id` to `(label, route)`.
pub fn build_rows(
    forwards: &[ForwardInfo],
    host: impl Fn(&str) -> (String, Vec<String>),
) -> Vec<ForwardRow> {
    forwards
        .iter()
        .map(|info| {
            let (host_label, route) = host(&info.spec.host_id);
            ForwardRow {
                info: info.clone(),
                host_label,
                route,
            }
        })
        .collect()
}

/// Rows of one host, as indices into the row slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostGroup {
    pub host_id: String,
    pub label: String,
    pub route: Vec<String>,
    pub rows: Vec<usize>,
}

/// Group rows by host in order of first appearance.
pub fn group_by_host(rows: &[ForwardRow]) -> Vec<HostGroup> {
    let mut groups: Vec<HostGroup> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        match groups
            .iter_mut()
            .find(|g| g.host_id == row.info.spec.host_id)
        {
            Some(group) => group.rows.push(index),
            None => groups.push(HostGroup {
                host_id: row.info.spec.host_id.clone(),
                label: row.host_label.clone(),
                route: row.route.clone(),
                rows: vec![index],
            }),
        }
    }
    groups
}

/// The action Enter triggers on a row.
pub fn primary_action(row: &ForwardRow) -> ForwardsUiEvent {
    let info = &row.info;
    let id = info.spec.id.clone();
    match info.state {
        ForwardState::AwaitingInput(_) => ForwardsUiEvent::Prompt(id),
        ForwardState::Failed { .. } | ForwardState::Stopped => ForwardsUiEvent::Restart(id),
        _ => match browser_url(info) {
            Some(url) => ForwardsUiEvent::OpenInBrowser(url),
            None => ForwardsUiEvent::CopyAddress(info.address_label()),
        },
    }
}

/// URL for "open in browser": only running local forwards to an HTTP-looking
/// target port.
pub fn browser_url(info: &ForwardInfo) -> Option<String> {
    if info.spec.kind != ForwardKind::Local
        || !info.is_running()
        || !looks_like_http(info.spec.target_port)
    {
        return None;
    }
    let scheme = if matches!(info.spec.target_port, 443 | 8443) {
        "https"
    } else {
        "http"
    };
    let address = info.address_label().replace("0.0.0.0", "127.0.0.1");
    Some(format!("{scheme}://{address}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use labonair_ssh::{ForwardId, ForwardLifetime, ForwardOrigin, ForwardSpec};

    pub(crate) fn info(id: &str, host: &str, state: ForwardState) -> ForwardInfo {
        ForwardInfo {
            spec: ForwardSpec {
                id: ForwardId(id.into()),
                host_id: host.into(),
                kind: ForwardKind::Local,
                name: None,
                listen_host: "127.0.0.1".into(),
                listen_port: 8080,
                target_host: "db".into(),
                target_port: 80,
            },
            state,
            lifetime: ForwardLifetime::Pinned,
            origin: ForwardOrigin::App,
            bound_port: None,
            connections: 0,
            bytes_in: 0,
            bytes_out: 0,
            saved: false,
        }
    }

    fn failed() -> ForwardState {
        ForwardState::Failed { reason: "x".into() }
    }

    #[test]
    fn health_worst_wins() {
        let a = info("a", "h", ForwardState::Active);
        let r = info("r", "h", ForwardState::Reconnecting { attempt: 1 });
        let f = info("f", "h", failed());
        assert_eq!(aggregate_health(&[]), Health::Active);
        assert_eq!(aggregate_health(std::slice::from_ref(&a)), Health::Active);
        assert_eq!(aggregate_health(&[a.clone(), r.clone()]), Health::Busy);
        assert_eq!(aggregate_health(&[a, r, f]), Health::Failed);
    }

    #[test]
    fn http_ports() {
        assert!(looks_like_http(8080) && looks_like_http(443));
        assert!(!looks_like_http(5432) && !looks_like_http(22));
    }

    #[test]
    fn state_texts() {
        assert_eq!(
            state_text(&ForwardState::Reconnecting { attempt: 2 }),
            "Reconnecting (2)"
        );
        assert_eq!(state_text(&failed()), "Failed: x");
        assert_eq!(state_text(&ForwardState::Active), "Active");
    }

    #[test]
    fn rows_group_in_stable_order() {
        let fs = [
            info("1", "b", ForwardState::Active),
            info("2", "a", ForwardState::Active),
            info("3", "b", ForwardState::Active),
        ];
        let rows = build_rows(&fs, |id| (id.to_uppercase(), vec!["j".into()]));
        assert_eq!(rows[0].host_label, "B");
        let groups = group_by_host(&rows);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].host_id, "b");
        assert_eq!(groups[0].rows, vec![0, 2]);
        assert_eq!(groups[1].rows, vec![1]);
    }

    #[test]
    fn primary_actions() {
        let row = |state| build_rows(&[info("a", "h", state)], |_| ("h".into(), vec![])).remove(0);
        assert_eq!(
            primary_action(&row(ForwardState::Active)),
            ForwardsUiEvent::OpenInBrowser("http://127.0.0.1:8080".into())
        );
        assert_eq!(
            primary_action(&row(failed())),
            ForwardsUiEvent::Restart(ForwardId("a".into()))
        );
        let mut r = row(ForwardState::Active);
        r.info.spec.target_port = 5432;
        assert_eq!(
            primary_action(&r),
            ForwardsUiEvent::CopyAddress("127.0.0.1:8080".into())
        );
    }
}
