//! Agent integrations: per-pane agent state polled from `sync_meta`, the tab
//! badge, and the "agent finished" notification. Recognition/heuristics are
//! the pure `labonair_terminal::agent` module.

use std::time::Duration;

use gpui::{
    percentage, Animation, AnimationExt, AnyElement, Context, IntoElement, Styled, Transformation,
};
use labonair_terminal::agent::{classify, Agent, AgentStatus, Tracker};
use labonair_ui_kit::{indicator, IconName, IndicatorSize, Palette};

use labonair_settings::Settings as _;

use crate::Workspace;

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
            // SSH panes have no foreground process: fall back to the screen.
            let is_ssh = self.ssh_tabs.values().any(|t| t.pane_id == id);
            let agent = match view.handle().foreground_process() {
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
                finished.push(agent);
            }
        }
        for agent in finished {
            if notify {
                let (title, body) = (agent.label(), "Finished its work");
                labonair_notifications::notify_os(title, body);
                labonair_notifications::notification_center(cx).update(cx, |center, cx| {
                    center.push(
                        labonair_notifications::Notification::success(title, body),
                        cx,
                    );
                });
            }
        }
        if changed {
            cx.notify();
        }
    }
}
