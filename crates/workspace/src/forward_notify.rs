//! Decides which Notifications-registry message (if any) a forward state
//! transition deserves. Pure; the workspace publishes and de-duplicates.

use labonair_ssh::ForwardState;

pub(crate) const SHOW_ACTION: &str = "forwards.show";
pub(crate) const AUTH_ACTION_PREFIX: &str = "forwards.authenticate:";
/// Text of the daemon version-gate error (`terminal-host` proxy).
pub(crate) const RESTART_HOST_HINT: &str = "Restart the background host";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ForwardNotice {
    Failed {
        reason: String,
    },
    /// Reconnect gave up (`Reconnecting` -> `Failed`).
    Lost {
        reason: String,
    },
    NeedsInput,
    /// Back to `Active`: resolve any open notice for this forward.
    Recovered,
}

impl ForwardNotice {
    /// Stable per-state-kind key: forward id + kind.
    pub(crate) fn key(&self, forward_id: &str) -> String {
        let kind = match self {
            Self::Failed { .. } => "failed",
            Self::Lost { .. } => "lost",
            Self::NeedsInput => "input",
            Self::Recovered => "active",
        };
        format!("forward:{forward_id}:{kind}")
    }
}

pub(crate) fn forward_notification(
    prev: Option<&ForwardState>,
    next: &ForwardState,
    user_started: bool,
) -> Option<ForwardNotice> {
    match next {
        ForwardState::Failed { reason } => match prev {
            Some(ForwardState::Failed { .. }) => None,
            Some(ForwardState::Reconnecting { .. }) => Some(ForwardNotice::Lost {
                reason: reason.clone(),
            }),
            _ => Some(ForwardNotice::Failed {
                reason: reason.clone(),
            }),
        },
        ForwardState::AwaitingInput(now) => {
            let seen = matches!(prev, Some(ForwardState::AwaitingInput(b)) if b.request_id == now.request_id);
            (!user_started && !seen).then_some(ForwardNotice::NeedsInput)
        }
        ForwardState::Active => match prev {
            Some(ForwardState::Active) | None => None,
            Some(_) => Some(ForwardNotice::Recovered),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use labonair_ssh::{PendingPrompt, PromptKind, PromptRequestId};

    fn failed() -> ForwardState {
        ForwardState::Failed {
            reason: "boom".into(),
        }
    }

    fn input(id: &str) -> ForwardState {
        ForwardState::AwaitingInput(PendingPrompt {
            request_id: PromptRequestId::new(id),
            kind: PromptKind::Password,
            hop: None,
        })
    }

    #[test]
    fn failed_and_give_up() {
        assert_eq!(
            forward_notification(Some(&ForwardState::Starting), &failed(), true),
            Some(ForwardNotice::Failed {
                reason: "boom".into()
            })
        );
        assert_eq!(
            forward_notification(
                Some(&ForwardState::Reconnecting { attempt: 3 }),
                &failed(),
                false
            ),
            Some(ForwardNotice::Lost {
                reason: "boom".into()
            })
        );
    }

    #[test]
    fn identical_states_do_not_spam() {
        assert_eq!(
            forward_notification(Some(&failed()), &failed(), false),
            None
        );
        assert_eq!(
            forward_notification(Some(&ForwardState::Active), &ForwardState::Active, false),
            None
        );
        assert_eq!(
            forward_notification(Some(&input("a")), &input("a"), false),
            None
        );
    }

    #[test]
    fn needs_input_only_when_not_user_started() {
        assert_eq!(
            forward_notification(Some(&ForwardState::Starting), &input("a"), false),
            Some(ForwardNotice::NeedsInput)
        );
        assert_eq!(forward_notification(None, &input("a"), true), None);
    }

    #[test]
    fn recovery_clears() {
        assert_eq!(
            forward_notification(Some(&failed()), &ForwardState::Active, false),
            Some(ForwardNotice::Recovered)
        );
        assert_eq!(
            forward_notification(None, &ForwardState::Active, false),
            None
        );
    }

    #[test]
    fn keys_differ_per_kind() {
        assert_ne!(
            ForwardNotice::NeedsInput.key("f"),
            ForwardNotice::Recovered.key("f")
        );
    }
}
