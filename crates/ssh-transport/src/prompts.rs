// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Prompt broker for sessions that have no tab-owned prompt UI (jump hops,
//! later forward sessions). ADR 0016 §2. The main-hop `TrustState` retry flow
//! is separate and unchanged.

use labonair_ssh::{HopInfo, PendingPrompt, PromptAnswer, PromptKind, PromptRequestId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

const PROMPT_TIMEOUT: Duration = Duration::from_secs(300);

/// Receives a prompt that must be shown to the user for `session_id`.
pub trait PromptSink: Send + Sync {
    fn publish(&self, session_id: &str, pending: PendingPrompt);
}

type Pending = Arc<Mutex<HashMap<PromptRequestId, oneshot::Sender<PromptAnswer>>>>;

#[derive(Clone, Default)]
pub struct PromptBroker(Pending);

impl PromptBroker {
    /// Publishes a prompt through `sink` and waits for [`Self::answer`].
    /// `None` = timed out (300 s) or the broker was dropped. The pending
    /// entry is always removed, including when this future is cancelled.
    pub async fn ask(
        &self,
        sink: &dyn PromptSink,
        session_id: &str,
        kind: PromptKind,
        hop: Option<HopInfo>,
    ) -> Option<PromptAnswer> {
        struct Cleanup<'a>(&'a Pending, PromptRequestId);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                if let Ok(mut m) = self.0.lock() {
                    m.remove(&self.1);
                }
            }
        }

        let request_id = PromptRequestId::new(uuid::Uuid::new_v4().to_string());
        let (tx, rx) = oneshot::channel();
        self.0.lock().ok()?.insert(request_id.clone(), tx);
        let _cleanup = Cleanup(&self.0, request_id.clone());
        sink.publish(
            session_id,
            PendingPrompt {
                request_id,
                kind,
                hop,
            },
        );
        tokio::time::timeout(PROMPT_TIMEOUT, rx).await.ok()?.ok()
    }

    /// Delivers an answer; `false` if the request is unknown or already gone.
    pub fn answer(&self, id: &PromptRequestId, answer: PromptAnswer) -> bool {
        let tx = self.0.lock().ok().and_then(|mut m| m.remove(id));
        tx.is_some_and(|tx| tx.send(answer).is_ok())
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.lock().map(|m| m.len()).unwrap_or(0)
    }
}

/// Publishes prompts as `hop_prompt` events on the application event bus; the
/// contract layer translates them to `SshConnectionEvent::HopPrompt`.
pub struct EventBusPromptSink(pub labonair_events::EventBus);

impl PromptSink for EventBusPromptSink {
    fn publish(&self, session_id: &str, pending: PendingPrompt) {
        let _ = self.0.emit(
            "hop_prompt",
            serde_json::json!({ "session_id": session_id, "pending": pending }),
        );
    }
}

/// Everything a connect needs to prompt interactively for its hops.
#[derive(Clone)]
pub struct HopPrompts {
    pub broker: PromptBroker,
    pub sink: Arc<dyn PromptSink>,
}

/// Prompt context of one hop: base (tab) session id plus the hop position.
#[derive(Clone)]
pub struct HopPromptCtx {
    pub prompts: HopPrompts,
    pub base_session_id: String,
    /// `None` for the main hop.
    pub info: Option<HopInfo>,
}

impl HopPromptCtx {
    pub async fn ask(&self, kind: PromptKind) -> Option<PromptAnswer> {
        self.prompts
            .broker
            .ask(
                &*self.prompts.sink,
                &self.base_session_id,
                kind,
                self.info.clone(),
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    struct ChanSink(mpsc::UnboundedSender<PendingPrompt>);
    impl PromptSink for ChanSink {
        fn publish(&self, _: &str, pending: PendingPrompt) {
            let _ = self.0.send(pending);
        }
    }

    #[tokio::test]
    async fn answer_delivers_and_cleans_up() {
        let broker = PromptBroker::default();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let b = broker.clone();
        let task = tokio::spawn(async move {
            b.ask(&ChanSink(tx), "tab", PromptKind::Password, None)
                .await
        });
        let pending = rx.recv().await.unwrap();
        assert!(broker.answer(&pending.request_id, PromptAnswer::Secret(Some("x".into()))));
        assert_eq!(
            task.await.unwrap(),
            Some(PromptAnswer::Secret(Some("x".into())))
        );
        assert_eq!(broker.len(), 0);
        assert!(!broker.answer(&pending.request_id, PromptAnswer::Trust(true)));
    }

    #[tokio::test(start_paused = true)]
    async fn times_out_after_300s() {
        let broker = PromptBroker::default();
        let (tx, _rx) = mpsc::unbounded_channel();
        let got = broker
            .ask(&ChanSink(tx), "tab", PromptKind::Passphrase, None)
            .await;
        assert_eq!(got, None);
        assert_eq!(broker.len(), 0);
    }

    #[tokio::test]
    async fn cancel_removes_entry() {
        let broker = PromptBroker::default();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let b = broker.clone();
        let task = tokio::spawn(async move {
            b.ask(&ChanSink(tx), "tab", PromptKind::Password, None)
                .await
        });
        rx.recv().await.unwrap();
        assert_eq!(broker.len(), 1);
        task.abort();
        let _ = task.await;
        assert_eq!(broker.len(), 0);
    }
}
