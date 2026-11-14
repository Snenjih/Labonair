//! Agent integrations: pure recognition of coding agents in a terminal and a
//! heuristic read of what they are doing. No I/O; the workspace polls the
//! foreground process, OSC title and visible tail and feeds them in here.
//!
//! Status heuristics (best effort, agents change their UI between releases):
//! * `NeedsInput`: the tail's last lines hold an approval / permission /
//!   question prompt (`Do you want to proceed?`, `[y/n]`, `Allow once`, ...).
//! * `Working`: the OSC title starts with a braille spinner glyph (Claude Code
//!   animates its title while busy) or the tail shows an interrupt hint
//!   (`esc to interrupt`, `esc interrupt`, `Working (`).
//! * otherwise the agent is idle at its prompt. Idle only counts as `Done`
//!   after work was seen and idle persisted for [`IDLE_POLLS_TO_FINISH`] polls,
//!   so a freshly started agent shows no badge and flicker never notifies.

use crate::resume::normalize_process_name;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Agent {
    ClaudeCode,
    Codex,
    Pi,
    OpenCode,
}

impl Agent {
    /// The agent behind a foreground process name (`/opt/bin/claude-2.1.0`).
    pub fn detect(process_name: &str) -> Option<Self> {
        match normalize_process_name(process_name).as_str() {
            "claude" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            "pi" => Some(Self::Pi),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }

    /// Key used by the `agentIntegration*` settings.
    pub fn key(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::OpenCode => "opencode",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Pi => "Pi",
            Self::OpenCode => "OpenCode",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentStatus {
    Working,
    NeedsInput,
    Done,
}

/// Raw reading of one poll: busy, blocked on the user, or idle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Raw {
    Working,
    NeedsInput,
    Idle,
}

const INPUT_HINTS: &[&str] = &[
    "do you want to",
    "[y/n]",
    "(y/n)",
    "allow once",
    "allow always",
    "allow this",
    "allow command",
    "yes, proceed",
    "permission required",
    "requires approval",
    "needs your approval",
    "waiting for your",
];

const WORK_HINTS: &[&str] = &["esc to interrupt", "esc interrupt", "ctrl+c to interrupt", "working ("];

/// Classify one poll from the OSC title and the visible tail (any length; only
/// the last 12 non-empty lines are looked at).
pub fn classify(title: &str, tail: &[String]) -> Raw {
    let lines: Vec<String> = tail
        .iter()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(12)
        .map(|l| l.to_lowercase())
        .collect();
    let any = |hints: &[&str]| lines.iter().any(|l| hints.iter().any(|h| l.contains(h)));
    if any(INPUT_HINTS) {
        return Raw::NeedsInput;
    }
    let spinner = title
        .trim_start()
        .chars()
        .next()
        .is_some_and(|c| ('\u{2800}'..='\u{28FF}').contains(&c));
    if spinner || any(WORK_HINTS) {
        Raw::Working
    } else {
        Raw::Idle
    }
}

/// Polls of consecutive idleness (~1 s each) before an agent counts as done.
pub const IDLE_POLLS_TO_FINISH: u8 = 2;

/// Debounces raw readings into the displayed status and reports the single
/// poll at which an agent finished.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tracker {
    status: Option<AgentStatus>,
    seen_work: bool,
    idle_polls: u8,
}

impl Tracker {
    /// Displayed status (`None` = no badge) and whether this poll is the
    /// moment the agent finished.
    pub fn update(&mut self, raw: Raw) -> (Option<AgentStatus>, bool) {
        let mut finished = false;
        match raw {
            Raw::Working | Raw::NeedsInput => {
                self.seen_work = true;
                self.idle_polls = 0;
                self.status = Some(if raw == Raw::Working {
                    AgentStatus::Working
                } else {
                    AgentStatus::NeedsInput
                });
            }
            Raw::Idle => {
                self.idle_polls = self.idle_polls.saturating_add(1);
                if self.seen_work
                    && self.idle_polls >= IDLE_POLLS_TO_FINISH
                    && self.status != Some(AgentStatus::Done)
                {
                    self.status = Some(AgentStatus::Done);
                    finished = true;
                }
            }
        }
        (self.status, finished)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tail(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn detects_agents_by_process_name() {
        assert_eq!(Agent::detect("claude"), Some(Agent::ClaudeCode));
        assert_eq!(Agent::detect("/opt/bin/Claude-2.1.0"), Some(Agent::ClaudeCode));
        assert_eq!(Agent::detect("codex"), Some(Agent::Codex));
        assert_eq!(Agent::detect("pi"), Some(Agent::Pi));
        assert_eq!(Agent::detect("opencode"), Some(Agent::OpenCode));
        assert_eq!(Agent::detect("vim"), None);
        assert_eq!(Agent::detect("pipx"), None);
    }

    #[test]
    fn classifies_title_and_tail() {
        assert_eq!(classify("\u{2802} Fix bug", &[]), Raw::Working);
        assert_eq!(classify("\u{2733} Claude Code", &[]), Raw::Idle);
        assert_eq!(classify("", &tail(&["* Thinking (esc to interrupt)"])), Raw::Working);
        assert_eq!(
            classify("", &tail(&["Do you want to proceed?", "> 1. Yes", "", ""])),
            Raw::NeedsInput
        );
        // A prompt wins over a stale spinner title.
        assert_eq!(classify("\u{2802} x", &tail(&["Allow once?"])), Raw::NeedsInput);
        assert_eq!(classify("", &tail(&["> ", ""])), Raw::Idle);
    }

    #[test]
    fn tracker_shows_nothing_until_work_then_done_after_debounce() {
        let mut t = Tracker::default();
        assert_eq!(t.update(Raw::Idle), (None, false));
        assert_eq!(t.update(Raw::Idle), (None, false));
        assert_eq!(t.update(Raw::Working), (Some(AgentStatus::Working), false));
        // One idle poll is flicker, not done.
        assert_eq!(t.update(Raw::Idle), (Some(AgentStatus::Working), false));
        assert_eq!(t.update(Raw::NeedsInput), (Some(AgentStatus::NeedsInput), false));
        assert_eq!(t.update(Raw::Idle), (Some(AgentStatus::NeedsInput), false));
        assert_eq!(t.update(Raw::Idle), (Some(AgentStatus::Done), true));
        // Finishing notifies once.
        assert_eq!(t.update(Raw::Idle), (Some(AgentStatus::Done), false));
        assert_eq!(t.update(Raw::Working), (Some(AgentStatus::Working), false));
    }
}
