# 0012 — Transfer terminal notifications

## Status

Accepted

## Context

`labonair-transfers-ui` owns transfer lifecycle presentation and already
receives typed completion, failure, and cancellation states. The statusbar
queue is the detailed progress and recovery surface, but it may be closed when
a transfer reaches a terminal state. The notifications center is the canonical
app-wide surface for user messages and supports retained, actionable toast
presentations.

Routing transfer outcomes through the shell would add feature-specific
behavior to a composition root. Keeping all terminal information only in the
queue can leave a failed or completed background transfer unnoticed.

## Decision

`labonair-transfers-ui` may depend on `labonair-notifications` to publish
terminal transfer outcomes. When its queue is closed, it publishes a retained
success or failure notification as an opt-in toast with an action that reveals
the same transfer queue. The queue remains the sole owner of detailed job
state, progress, conflicts, and recovery decisions. It does not publish a
second toast while already open.

## Consequences

- Transfer outcomes use the shared notification registry and toast overlay.
- The action is data-only and is handled by the transfer UI owner.
- Transfer progress, conflict prompts, and file-error recovery stay in the
  existing transfer UI; the notifications center does not mirror that state.
- `labonair-transfers-ui` adds one dependency on `labonair-notifications`.
