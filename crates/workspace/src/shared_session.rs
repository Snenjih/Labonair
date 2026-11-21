// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Stable Workspace-owned tab payloads for the multi-process Workspace store.
//!
//! The existing restore format uses process-local `u64` pane handles. Those
//! handles stay useful to GPUI and the pane registries, but are translated to
//! stable UUIDs at the persistence boundary. Focus and active-pane selection
//! remain local to each app window.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pane::SplitAxis;
use crate::session_v3::{PaneGroupSnapshot, SerializedNode, SerializedPane, SerializedTree};
use labonair_workspace_storage::PaneId;

const SHARED_TAB_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SharedTabPayload {
    pub version: u32,
    #[serde(default)]
    pub title: Option<String>,
    pub tree: SharedTree,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SharedTree {
    #[serde(default)]
    pub root: Option<SharedNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum SharedNode {
    Pane {
        id: PaneId,
        #[serde(flatten)]
        pane: SerializedPane,
    },
    Axis {
        axis: SplitAxis,
        members: Vec<SharedNode>,
        #[serde(default)]
        flexes: Vec<f32>,
    },
    #[serde(other)]
    Unknown,
}

impl SharedTabPayload {
    /// Migrate one v3 tab, minting stable IDs once for every durable pane.
    pub(crate) fn from_legacy(group: &PaneGroupSnapshot) -> Self {
        Self {
            version: SHARED_TAB_VERSION,
            title: group.title.clone(),
            tree: SharedTree {
                root: group
                    .tree
                    .root
                    .as_ref()
                    .map(|node| migrate_node(node, &mut HashMap::new())),
            },
        }
    }

    /// Capture a current runtime tree, reusing each pane's stable ID.
    pub(crate) fn capture(
        group: &PaneGroupSnapshot,
        mut stable_id: impl FnMut(u64) -> PaneId,
    ) -> Self {
        Self {
            version: SHARED_TAB_VERSION,
            title: group.title.clone(),
            tree: SharedTree {
                root: group
                    .tree
                    .root
                    .as_ref()
                    .map(|node| capture_node(node, &mut stable_id)),
            },
        }
    }

    /// Adapt stable storage IDs back to the existing defensive restore
    /// pipeline. `temporary_to_stable` is later paired with `restore_tree`'s
    /// remap so this process can retain the stable identities of its new
    /// process-local pane handles.
    pub(crate) fn to_runtime_group(&self) -> Option<(PaneGroupSnapshot, HashMap<u64, PaneId>)> {
        if self.version != SHARED_TAB_VERSION {
            return None;
        }
        let mut next_id = 1_u64;
        let mut temporary_to_stable = HashMap::new();
        let root = self
            .tree
            .root
            .as_ref()
            .map(|node| runtime_node(node, &mut next_id, &mut temporary_to_stable));
        Some((
            PaneGroupSnapshot {
                title: self.title.clone(),
                tree: SerializedTree { root, active: None },
            },
            temporary_to_stable,
        ))
    }

    pub(crate) fn scrollback_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        if let Some(root) = &self.tree.root {
            collect_scrollback(root, &mut keys);
        }
        keys.sort();
        keys.dedup();
        keys
    }

    pub(crate) fn into_value(self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value)
    }
}

fn migrate_node(node: &SerializedNode, old_to_stable: &mut HashMap<u64, PaneId>) -> SharedNode {
    match node {
        SerializedNode::Pane { id, pane } => SharedNode::Pane {
            id: *old_to_stable.entry(*id).or_default(),
            pane: pane.clone(),
        },
        SerializedNode::Axis {
            axis,
            members,
            flexes,
            ..
        } => SharedNode::Axis {
            axis: *axis,
            members: members
                .iter()
                .map(|member| migrate_node(member, old_to_stable))
                .collect(),
            flexes: flexes.clone(),
        },
        SerializedNode::Unknown => SharedNode::Unknown,
    }
}

fn capture_node(node: &SerializedNode, stable_id: &mut impl FnMut(u64) -> PaneId) -> SharedNode {
    match node {
        SerializedNode::Pane { id, pane } => SharedNode::Pane {
            id: stable_id(*id),
            pane: pane.clone(),
        },
        SerializedNode::Axis {
            axis,
            members,
            flexes,
            ..
        } => SharedNode::Axis {
            axis: *axis,
            members: members
                .iter()
                .map(|member| capture_node(member, stable_id))
                .collect(),
            flexes: flexes.clone(),
        },
        SerializedNode::Unknown => SharedNode::Unknown,
    }
}

fn runtime_node(
    node: &SharedNode,
    next_id: &mut u64,
    temporary_to_stable: &mut HashMap<u64, PaneId>,
) -> SerializedNode {
    match node {
        SharedNode::Pane { id, pane } => {
            let temporary = *next_id;
            *next_id = next_id.saturating_add(1);
            temporary_to_stable.insert(temporary, *id);
            SerializedNode::Pane {
                id: temporary,
                pane: pane.clone(),
            }
        }
        SharedNode::Axis {
            axis,
            members,
            flexes,
        } => SerializedNode::Axis {
            id: 0,
            axis: *axis,
            members: members
                .iter()
                .map(|member| runtime_node(member, next_id, temporary_to_stable))
                .collect(),
            flexes: flexes.clone(),
        },
        SharedNode::Unknown => SerializedNode::Unknown,
    }
}

fn collect_scrollback(node: &SharedNode, keys: &mut Vec<String>) {
    match node {
        SharedNode::Pane { pane, .. } if pane.kind == "terminal" => {
            if let Some(key) = pane.state.get("scrollbackId").and_then(Value::as_str) {
                keys.push(key.to_string());
            }
        }
        SharedNode::Axis { members, .. } => {
            for member in members {
                collect_scrollback(member, keys);
            }
        }
        SharedNode::Pane { .. } | SharedNode::Unknown => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_v3::{SerializedPane, SerializedTree};

    fn group() -> PaneGroupSnapshot {
        PaneGroupSnapshot {
            title: Some("Shells".into()),
            tree: SerializedTree {
                root: Some(SerializedNode::Axis {
                    id: 55,
                    axis: SplitAxis::Horizontal,
                    members: vec![SerializedNode::Pane {
                        id: 12,
                        pane: SerializedPane {
                            kind: "terminal".into(),
                            state: serde_json::json!({
                                "kind": "local",
                                "scrollbackId": "scrollback-a"
                            }),
                        },
                    }],
                    flexes: vec![1.0],
                }),
                active: Some(12),
            },
        }
    }

    #[test]
    fn persisted_pane_ids_are_stable_uuids_and_runtime_handles_are_remapped() {
        let shared = SharedTabPayload::from_legacy(&group());
        let stable_id = match shared.tree.root.as_ref().unwrap() {
            SharedNode::Axis { members, .. } => match &members[0] {
                SharedNode::Pane { id, .. } => *id,
                _ => panic!("expected a pane leaf"),
            },
            _ => panic!("expected a split tree"),
        };
        let (runtime, mapping) = shared.to_runtime_group().unwrap();
        assert_eq!(mapping.get(&1), Some(&stable_id));
        assert_eq!(runtime.title.as_deref(), Some("Shells"));
        assert!(runtime.tree.active.is_none());
        assert_eq!(shared.scrollback_keys(), vec!["scrollback-a"]);
    }
}
