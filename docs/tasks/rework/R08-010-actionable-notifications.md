# R08-010 — Actionable notifications with stable action ids

## Status

In progress. Activated by the Git Source Control publish-to-PR workflow.

## Contract

`labonair-notifications-core` keeps data-only actions and retained notification
records. Owner actions emit a typed event; typed external-URL actions are
handled by the notifications adapter. A notification can opt into a temporary
toast while remaining in the statusbar dropdown. See [ADR 0011](../../adr/0011-actionable-notification-presentations.md).

## Implemented

- The dropdown renders notification actions and groups Problems separately
  from Activity.
- The Git Source Control `Publish` action looks up `origin`'s push URL and,
  after a successful push, offers a GitHub pull-request action when the remote
  is a recognized GitHub repository.
- The PR action opens GitHub's branch-specific `/pull/new/<branch>` page.

## Remaining

- Add focused publish-to-action and toast-lifecycle tests.
- Visually inspect the open dropdown and actionable toast in the running native
  app.
