# 0014: Native OS notifications and fixed toast lifetime

## Status

Accepted

## Context

OS notifications were delivered by spawning `osascript`, which attributes them to Script Editor (no app icon) and opens a script host on click. ADR 0011 toasts also used per-caller durations and appeared at the top of the window.

## Decision

- macOS delivery uses `UNUserNotificationCenter` from `labonair-notifications` (`os_notify.rs`), so notifications carry the app bundle identity (`com.labonair.rust`) and icon. Authorization is requested lazily on first delivery. Unbundled runs (no `.app` bundle) log once and skip delivery; no script host is ever spawned.
- A click activates the app. If the notification carries an opaque owner target string, it is passed to a handler registered with `set_os_click_handler`; the composition root maps it to tab/pane/file navigation. Without a target the app is only brought to front.
- All in-app toasts live `TOAST_DURATION` (15 s, `labonair-notifications-core`). `toast_for(d)` only opts into a toast. Retained records are unaffected.
- Toasts render at the window bottom above the statusbar, using the shared popover radius token (`palette.radius.lg`).
- New macOS-only dependencies on `labonair-notifications`: `objc2-user-notifications`, `block2` (already in the tree via gpui/objc2); `objc2`, `objc2-foundation`, `objc2-app-kit` already existed elsewhere.

## Consequences

- Real delivery and click routing work only from a bundled app and are not covered by automated tests; pure routing and expiry logic are unit-tested.
- Wiring a concrete navigation handler per feature is follow-up work for the owning crates.
- Linux/Windows native delivery remains unimplemented.
