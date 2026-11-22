# ADR 0011: Actionable notification presentations

## Status

Accepted

## Context

Notifications currently live in one retained registry and are shown only in a
statusbar dropdown. The Git Source Control panel already publishes a new local
branch to `origin`, but it cannot offer a follow-up action for creating a pull
request. The current dropdown also lacks search and visual grouping, making
errors, information, and available actions harder to scan.

The product contract says there is no toast surface. The requested Git workflow
needs a brief, actionable success prompt while keeping the notification
available in the existing history. A separate feature-local banner or a
second notification owner would duplicate state and action handling.

## Decision

- `labonair-notifications-core` remains the single owner of retained
  notifications. A notification may opt into a transient toast presentation
  with a caller-selected duration; the retained entry and its actions remain
  in the dropdown after the toast expires or is closed.
- `labonair-notifications` owns the toast renderer and statusbar dropdown.
  The shell composes the notification-owned toast overlay at the app root.
  Toasts are opt-in and do not change the default retained-notification path.
- The dropdown groups errors and warnings under Problems and other entries
  under Activity. It provides an expanding search control, an icon-only
  clear-all action, severity cues, and the existing data-only action model.
- Notification actions may carry a typed external-URL target. The notification
  UI opens that target; other actions continue to emit their stable action ID
  to the owning module.
- `labonair-git::GitService` exposes remote URL lookup through its existing
  transport adapter. After a successful first publish, Source Control creates
  a GitHub `/pull/new/<branch>` URL only when `origin` is a recognized GitHub
  remote, then publishes a success toast with an Open PR action. Other remotes
  receive the success toast without a PR action.

## Consequences

- Notifications remain the single user-message path; toast content is derived
  from the retained notification record and is not a duplicate message store.
- Toast timers and dismissal affect transient visibility only, not unread state
  or retention.
- Feature modules can request a toast through the notification builder without
  depending on a toast view or registering feature-specific UI.
- Git remote lookup becomes part of the typed Git capability contract and
  continues to work for local and SSH-backed repositories.
- The product's existing no-toast statement is replaced by this opt-in
  presentation contract.
