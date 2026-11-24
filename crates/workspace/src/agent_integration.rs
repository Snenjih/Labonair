// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Agent integrations: per-pane agent state polled from `sync_meta`, the tab
//! badge, and the "agent finished" notification. Recognition/heuristics are
//! the pure `labonair_terminal::agent` module.

use std::{collections::HashSet, time::Duration};

use gpui::{
    percentage, Animation, AnimationExt, AnyElement, Context, IntoElement, Styled, Transformation,
};
use labonair_terminal::agent::{classify, Agent, AgentStatus, Tracker};
use labonair_ui_kit::{indicator, IconName, IndicatorSize, Palette};

use labonair_settings::Settings as _;

use crate::Workspace;

const PANE_TARGET_PREFIX: &str = "pane:";

/// OS-notification click target identifying a terminal pane.
pub(crate) fn encode_pane_target(pane: crate::pane::PaneId) -> String {
    format!("{PANE_TARGET_PREFIX}{pane}")
}

pub(crate) fn decode_pane_target(target: &str) -> Option<crate::pane::PaneId> {
    target.strip_prefix(PANE_TARGET_PREFIX)?.parse().ok()
}

pub(crate) const OPEN_RESULT_ACTION: &str = "agent.open-result";

/// Live agent state of one terminal pane.
pub(crate) struct AgentPane {
    pub agent: Agent,
    pub status: Option<AgentStatus>,
    tracker: Tracker,
}

pub(crate) fn icon(agent: Agent) -> IconName {
    match agent {
        Agent::ClaudeCode => IconName::AiClaude,
        Agent::Codex => IconName::AiOpenAi,
        Agent::Pi => IconName::AgentPi,
        Agent::OpenCode => IconName::AiOpenCode,
    }
}

/// Status badge after the tab title: spinner / yellow "!" / blue dot.
pub(crate) fn badge(status: AgentStatus, c: &Palette) -> AnyElement {
    match status {
        AgentStatus::Working => IconName::LoadCircle
            .svg(c.muted)
            .size(gpui::px(12.0))
            .with_animation(
                "agent-spinner",
                Animation::new(Duration::from_secs(2)).repeat(),
                |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
            )
            .into_any_element(),
        AgentStatus::NeedsInput => IconName::Warning
            .svg(c.warning)
            .size(gpui::px(13.0))
            .into_any_element(),
        AgentStatus::Done => indicator(IndicatorSize::Xs, c.info).into_any_element(),
    }
}

impl Workspace {
    /// The agent to show for a tab: the pane with the most urgent status
    /// (NeedsInput > Working > Done > no badge) among all of the tab's panes.
    pub(crate) fn tab_agent(&self, tab_id: u64) -> Option<&AgentPane> {
        let rank = |a: &AgentPane| match a.status {
            Some(AgentStatus::NeedsInput) => 3,
            Some(AgentStatus::Working) => 2,
            Some(AgentStatus::Done) => 1,
            None => 0,
        };
        self.layouts
            .get(&tab_id)?
            .leaves()
            .into_iter()
            .filter_map(|p| self.agents.get(&p))
            .max_by_key(|a| rank(a))
    }

    /// Poll every terminal pane's foreground process / title / tail. Called
    /// from the 400 ms meta sync; cheap for panes without an agent.
    pub(crate) fn sync_agents(&mut self, cx: &mut Context<Self>) {
        let settings = labonair_settings::TerminalSettings::try_get(cx).cloned();
        let enabled = |a: Agent| {
            settings
                .as_ref()
                .is_none_or(|s| s.agent_integration_enabled(a.key()))
        };
        let notify = settings
            .as_ref()
            .is_none_or(|s| s.agent_integration_notify());
        let mut changed = false;
        let mut finished = Vec::new();
        let ids: Vec<_> = self.panes.keys().copied().collect();
        for id in ids {
            let view = self.panes[&id].view.read(cx);
            // Foreground program tile (node, python, htop ...); idle shell = none.
            let foreground = view.handle().foreground_process();
            let program = foreground
                .as_ref()
                .and_then(|p| crate::program_tile::program_tile(&p.name));
            match program {
                Some(tile) => changed |= self.programs.insert(id, tile) != Some(tile),
                None => changed |= self.programs.remove(&id).is_some(),
            }
            // SSH panes have no foreground process: fall back to the screen.
            let is_ssh = self.ssh_tabs.values().any(|t| t.pane_id == id);
            let agent = match foreground {
                Some(p) => Agent::detect(&p.name),
                None if is_ssh => Agent::detect_screen(
                    &view.shell_title().unwrap_or_default(),
                    &view.recent_lines(30),
                ),
                None => None,
            }
            .filter(|a| enabled(*a));
            let Some(agent) = agent else {
                changed |= self.agents.remove(&id).is_some();
                continue;
            };
            let raw = classify(
                &view.shell_title().unwrap_or_default(),
                &view.recent_lines(30),
            );
            let entry = self.agents.entry(id).or_insert_with(|| {
                changed = true;
                AgentPane {
                    agent,
                    status: None,
                    tracker: Tracker::default(),
                }
            });
            if entry.agent != agent {
                *entry = AgentPane {
                    agent,
                    status: None,
                    tracker: Tracker::default(),
                };
                changed = true;
            }
            let (status, done) = entry.tracker.update(raw);
            changed |= entry.status != status;
            entry.status = status;
            if done {
                finished.push((id, agent));
            }
        }
        for (pane_id, agent) in finished {
            if notify {
                let (title, body) = (agent.label(), "Finished its work");
                if cx.active_window().is_none() {
                    labonair_notifications::notify_os_with_target(
                        title,
                        body,
                        Some(&encode_pane_target(pane_id)),
                    );
                }
                let center = labonair_notifications::notification_center(cx);
                let notification_id = center.update(cx, |center, cx| {
                    center.push(
                        labonair_notifications::Notification::success(title, body)
                            .source("agent")
                            .dedupe_key(format!("agent:finished:{pane_id:?}"))
                            .action(OPEN_RESULT_ACTION, "Open result")
                            .toast_for(Duration::from_secs(8)),
                        cx,
                    )
                });
                let retained_ids: HashSet<_> = center
                    .read(cx)
                    .snapshots()
                    .into_iter()
                    .map(|notification| notification.id)
                    .collect();
                self.agent_notification_targets
                    .retain(|id, _| retained_ids.contains(id));
                if let Some(notification_id) = notification_id {
                    self.agent_notification_targets
                        .insert(notification_id, pane_id);
                }
            }
        }
        if changed {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod target_tests {
    use super::*;

    #[test]
    fn pane_target_roundtrips() {
        assert_eq!(decode_pane_target(&encode_pane_target(42)), Some(42));
        assert_eq!(decode_pane_target("file:/x"), None);
        assert_eq!(decode_pane_target("pane:x"), None);
    }
}
