// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Palette contributions of the forwards owner (ADR 0016 §4): command
//! metadata, pure submenu builders, the query-aware second-step rows and the
//! typed decoding of palette actions into [`ForwardsUiEvent`]s. GPUI-free.

use labonair_command_palette_core::{
    CommandDescriptor, CommandIcon, CommandId, CommandProvider, CommandSubmenu, SubmenuAction,
    SubmenuDescriptor, SubmenuItem, SubmenuSnapshot,
};
use labonair_ssh::{ForwardId, ForwardInfo};

use crate::{parse_quick, route_label, QuickSpec};

/// Typed request the forwards palette (and, later, the forwards UI) hands to
/// the workspace, which performs it through `SshForwardService`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardsUiEvent {
    New {
        host_id: Option<String>,
        quick: Option<QuickSpec>,
    },
    Close(ForwardId),
    Restart(ForwardId),
    /// Start or stop a saved forward.
    Toggle(ForwardId),
    StopAll,
    Manage {
        host_id: Option<String>,
    },
    EditRoute {
        host_id: String,
    },
    /// Copy a forward's listen address to the clipboard.
    CopyAddress(String),
    /// Open a forward's address in the default browser.
    OpenInBrowser(String),
    /// Answer the pending prompt of an `AwaitingInput` forward.
    Prompt(ForwardId),
}

/// What a forwards/jump palette action asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardsIntent {
    Ui(ForwardsUiEvent),
    /// Open SSH to `host_id` through a one-off route (outermost hop first).
    OpenSshVia {
        host_id: String,
        route: Vec<String>,
    },
}

/// Decodes a submenu action owned by this module; `None` = not ours.
pub fn decode_action(action: &SubmenuAction) -> Option<ForwardsIntent> {
    Some(match action {
        SubmenuAction::OpenForwardEditor { host_id, quick } => {
            ForwardsIntent::Ui(ForwardsUiEvent::New {
                host_id: host_id.clone(),
                quick: quick.as_deref().and_then(|q| parse_quick(q).ok()),
            })
        }
        SubmenuAction::ToggleForward { forward_id } => {
            ForwardsIntent::Ui(ForwardsUiEvent::Toggle(ForwardId(forward_id.clone())))
        }
        SubmenuAction::CloseForward { id } => {
            ForwardsIntent::Ui(ForwardsUiEvent::Close(ForwardId(id.clone())))
        }
        SubmenuAction::EditJumpRoute { host_id } => {
            ForwardsIntent::Ui(ForwardsUiEvent::EditRoute {
                host_id: host_id.clone(),
            })
        }
        SubmenuAction::OpenSshVia { host_id, route } => ForwardsIntent::OpenSshVia {
            host_id: host_id.clone(),
            route: route.clone(),
        },
        _ => return None,
    })
}

/// A host the user can pick (mirrors the Hosts picker row without depending
/// on the Hosts crate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostChoice {
    pub id: String,
    pub name: String,
    pub subtitle: String,
}

/// A forward plus the display name of its host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardRow {
    pub info: ForwardInfo,
    pub host_name: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ForwardsCommandProvider;

impl CommandProvider for ForwardsCommandProvider {
    fn commands(&self) -> Vec<CommandDescriptor> {
        let c = |id, title, submenu: Option<CommandSubmenu>| {
            let d = CommandDescriptor::new(id, title, "Connections").with_icon(CommandIcon::Server);
            match submenu {
                Some(s) => d.with_submenu(s),
                None => d,
            }
        };
        vec![
            c(
                CommandId::ForwardsNew,
                "Port Forwards: New…",
                Some(CommandSubmenu::ForwardsHosts),
            ),
            c(
                CommandId::ForwardsStartSaved,
                "Port Forwards: Start Saved…",
                Some(CommandSubmenu::ForwardsSaved),
            ),
            c(
                CommandId::ForwardsStop,
                "Port Forwards: Stop…",
                Some(CommandSubmenu::ForwardsActive),
            ),
            c(CommandId::ForwardsStopAll, "Port Forwards: Stop All", None),
            c(CommandId::ForwardsManage, "Port Forwards: Manage", None),
            c(
                CommandId::JumpEditRoute,
                "Jump Hosts: Edit Route…",
                Some(CommandSubmenu::RouteHosts),
            ),
            c(
                CommandId::OpenSshVia,
                "Open SSH via…",
                Some(CommandSubmenu::ViaTargets),
            ),
        ]
    }
}

fn host_picker(
    id: &str,
    title: &str,
    submenu: CommandSubmenu,
    hosts: &[HostChoice],
    action: impl Fn(&HostChoice) -> SubmenuAction,
) -> SubmenuSnapshot {
    SubmenuSnapshot {
        descriptor: SubmenuDescriptor::new(id, title, submenu),
        items: hosts
            .iter()
            .map(|h| SubmenuItem {
                id: h.id.clone(),
                title: h.name.clone(),
                subtitle: Some(h.subtitle.clone()),
                active: false,
                action: action(h),
                secondary: None,
            })
            .collect(),
    }
}

fn drill(submenu: CommandSubmenu, h: &HostChoice) -> SubmenuAction {
    SubmenuAction::Drill {
        submenu,
        carry: h.id.clone(),
    }
}

/// Step 1 of `Port Forwards: New…` (host first, then the spec step).
pub fn forwards_hosts_submenu(hosts: &[HostChoice]) -> SubmenuSnapshot {
    host_picker(
        "forwards-hosts",
        "New Port Forward",
        CommandSubmenu::ForwardsHosts,
        hosts,
        |h| drill(CommandSubmenu::ForwardsQuick, h),
    )
}

/// `Jump Hosts: Edit Route…`.
pub fn route_hosts_submenu(hosts: &[HostChoice]) -> SubmenuSnapshot {
    host_picker(
        "route-hosts",
        "Jump Route",
        CommandSubmenu::RouteHosts,
        hosts,
        |h| SubmenuAction::EditJumpRoute {
            host_id: h.id.clone(),
        },
    )
}

/// Step 1 of `Open SSH via…` (target host).
pub fn via_targets_submenu(hosts: &[HostChoice]) -> SubmenuSnapshot {
    host_picker(
        "via-targets",
        "Open SSH via",
        CommandSubmenu::ViaTargets,
        hosts,
        |h| drill(CommandSubmenu::ViaJumps, h),
    )
}

fn forward_item(row: &ForwardRow, action: SubmenuAction) -> SubmenuItem {
    let spec = &row.info.spec;
    SubmenuItem {
        id: spec.id.0.clone(),
        title: spec
            .name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| route_label(spec)),
        subtitle: Some(format!("{} · {}", row.host_name, route_label(spec))),
        active: row.info.is_running(),
        action,
        secondary: None,
    }
}

/// `Port Forwards: Start Saved…` — saved forwards; `active` = running.
pub fn saved_forwards_submenu(rows: &[ForwardRow]) -> SubmenuSnapshot {
    SubmenuSnapshot {
        descriptor: SubmenuDescriptor::new(
            "forwards-saved",
            "Saved Port Forwards",
            CommandSubmenu::ForwardsSaved,
        ),
        items: rows
            .iter()
            .filter(|r| r.info.saved)
            .map(|r| {
                forward_item(
                    r,
                    SubmenuAction::ToggleForward {
                        forward_id: r.info.spec.id.0.clone(),
                    },
                )
            })
            .collect(),
    }
}

/// `Port Forwards: Stop…` — running forwards only.
pub fn active_forwards_submenu(rows: &[ForwardRow]) -> SubmenuSnapshot {
    SubmenuSnapshot {
        descriptor: SubmenuDescriptor::new(
            "forwards-active",
            "Active Port Forwards",
            CommandSubmenu::ForwardsActive,
        ),
        items: rows
            .iter()
            .filter(|r| r.info.is_running())
            .map(|r| {
                forward_item(
                    r,
                    SubmenuAction::CloseForward {
                        id: r.info.spec.id.0.clone(),
                    },
                )
            })
            .collect(),
    }
}

fn matches_query(h: &HostChoice, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || h.name.to_lowercase().contains(&q) || h.subtitle.to_lowercase().contains(&q)
}

/// Query-aware rows of the second step. `carry` is the host picked in step 1.
/// - `ForwardsQuick`: the parsed quick spec row (`Forward -L 8080:db:5432 on
///   <host>`) when the query parses, then an editor row.
/// - `ViaJumps`: every other host as the jump (single hop; the data type is a
///   route so more hops can be added later).
pub fn dynamic_rows(
    submenu: CommandSubmenu,
    carry: &str,
    query: &str,
    hosts: &[HostChoice],
) -> Vec<SubmenuItem> {
    let host_name = hosts
        .iter()
        .find(|h| h.id == carry)
        .map_or(carry, |h| h.name.as_str());
    match submenu {
        CommandSubmenu::ForwardsQuick => {
            let mut rows = Vec::new();
            let query = query.trim();
            if !query.is_empty() && parse_quick(query).is_ok() {
                rows.push(SubmenuItem {
                    id: "quick".into(),
                    title: format!("Forward {query} on {host_name}"),
                    subtitle: None,
                    active: false,
                    action: SubmenuAction::OpenForwardEditor {
                        host_id: Some(carry.to_string()),
                        quick: Some(query.to_string()),
                    },
                    secondary: None,
                });
            }
            rows.push(SubmenuItem {
                id: "editor".into(),
                title: format!("Custom forward on {host_name}…"),
                subtitle: Some("Type -L, -R or -D to use the ssh quick syntax".into()),
                active: false,
                action: SubmenuAction::OpenForwardEditor {
                    host_id: Some(carry.to_string()),
                    quick: None,
                },
                secondary: None,
            });
            rows
        }
        CommandSubmenu::ViaJumps => hosts
            .iter()
            .filter(|h| h.id != carry && matches_query(h, query))
            .map(|h| SubmenuItem {
                id: h.id.clone(),
                title: format!("{host_name} via {}", h.name),
                subtitle: Some(h.subtitle.clone()),
                active: false,
                action: SubmenuAction::OpenSshVia {
                    host_id: carry.to_string(),
                    route: vec![h.id.clone()],
                },
                secondary: None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use labonair_ssh::{ForwardKind, ForwardLifetime, ForwardOrigin, ForwardSpec, ForwardState};

    fn hosts() -> Vec<HostChoice> {
        ["a", "b"]
            .iter()
            .map(|id| HostChoice {
                id: id.to_string(),
                name: id.to_uppercase(),
                subtitle: format!("u@{id}:22"),
            })
            .collect()
    }

    fn row(id: &str, state: ForwardState, saved: bool) -> ForwardRow {
        ForwardRow {
            host_name: "A".into(),
            info: ForwardInfo {
                spec: ForwardSpec {
                    id: ForwardId(id.into()),
                    host_id: "a".into(),
                    kind: ForwardKind::Local,
                    name: None,
                    listen_host: "127.0.0.1".into(),
                    listen_port: 8080,
                    target_host: "db".into(),
                    target_port: 5432,
                },
                state,
                lifetime: ForwardLifetime::Pinned,
                origin: ForwardOrigin::App,
                bound_port: None,
                connections: 0,
                bytes_in: 0,
                bytes_out: 0,
                saved,
            },
        }
    }

    #[test]
    fn provider_lists_the_seven_commands() {
        let cmds = ForwardsCommandProvider.commands();
        let titles: Vec<_> = cmds.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles.len(), 7);
        assert!(titles.contains(&"Open SSH via…") && titles.contains(&"Port Forwards: Stop All"));
        assert!(cmds.iter().all(|c| c.section == "Connections"));
        assert!(cmds.iter().all(|c| c.default_bindings.is_empty()));
    }

    #[test]
    fn step_one_pickers_drill_or_act() {
        let new = forwards_hosts_submenu(&hosts());
        assert_eq!(
            new.items[1].action,
            SubmenuAction::Drill {
                submenu: CommandSubmenu::ForwardsQuick,
                carry: "b".into()
            }
        );
        assert_eq!(
            route_hosts_submenu(&hosts()).items[0].action,
            SubmenuAction::EditJumpRoute {
                host_id: "a".into()
            }
        );
    }

    #[test]
    fn saved_and_active_submenus_filter_and_mark_running() {
        let rows = [
            row("s1", ForwardState::Active, true),
            row("s2", ForwardState::Stopped, true),
            row("p1", ForwardState::Active, false),
        ];
        let saved = saved_forwards_submenu(&rows);
        assert_eq!(saved.items.len(), 2);
        assert!(saved.items[0].active && !saved.items[1].active);
        assert_eq!(
            saved.items[0].subtitle.as_deref(),
            Some("A · 127.0.0.1:8080 → db:5432")
        );
        let active = active_forwards_submenu(&rows);
        assert_eq!(active.items.len(), 2);
        assert_eq!(
            active.items[1].action,
            SubmenuAction::CloseForward { id: "p1".into() }
        );
    }

    #[test]
    fn quick_syntax_shows_parsed_row_after_host_pick() {
        let rows = dynamic_rows(
            CommandSubmenu::ForwardsQuick,
            "a",
            "-L 8080:db:5432",
            &hosts(),
        );
        assert_eq!(rows[0].title, "Forward -L 8080:db:5432 on A");
        let decoded = decode_action(&rows[0].action).unwrap();
        let ForwardsIntent::Ui(ForwardsUiEvent::New { host_id, quick }) = decoded else {
            panic!("expected New");
        };
        assert_eq!(host_id.as_deref(), Some("a"));
        assert_eq!(quick.unwrap().target_port, 5432);
        // Unparseable text only offers the editor row.
        let rows = dynamic_rows(CommandSubmenu::ForwardsQuick, "a", "-L nope", &hosts());
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn via_jumps_excludes_target_and_decodes_to_route() {
        let rows = dynamic_rows(CommandSubmenu::ViaJumps, "a", "", &hosts());
        assert_eq!(rows.len(), 1);
        assert_eq!(
            decode_action(&rows[0].action),
            Some(ForwardsIntent::OpenSshVia {
                host_id: "a".into(),
                route: vec!["b".into()]
            })
        );
        assert!(dynamic_rows(CommandSubmenu::ViaJumps, "a", "zzz", &hosts()).is_empty());
    }

    #[test]
    fn decode_ignores_foreign_actions() {
        assert_eq!(decode_action(&SubmenuAction::SwitchToTab(1)), None);
        assert_eq!(
            decode_action(&SubmenuAction::CloseForward { id: "f".into() }),
            Some(ForwardsIntent::Ui(ForwardsUiEvent::Close(ForwardId(
                "f".into()
            ))))
        );
    }
}
