# R08-010 — Actionable notifications with stable action ids

## Status

Deferred (no consumer). Activate only when a concrete feature needs an actionable notification.

## Statement

`labonair-notifications-core` keeps a data-only `NotificationAction` (id + label); no closures are stored (R08-009). Left to do when activated: render the action in the statusbar dropdown, emit `action_id` + notification id as a typed event, and route it through the owning module's command/palette handler registry, with tests for publish-to-action round trip. Nothing is to be built before a real consumer exists.
