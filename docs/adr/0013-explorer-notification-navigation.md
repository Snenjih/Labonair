# 0013 — Explorer notification navigation

## Status

Accepted

## Context

Explorer file operations can finish after the user has moved away from the
Explorer dock. Their retained Notifications toast can offer an `Open
destination` action, but the Explorer view does not own dock visibility. The
Workspace owns dock activation, and feature-to-composition communication uses
typed events rather than feature-specific shell behavior.

## Decision

When the Explorer notification action is activated, the Explorer owner loads
the recorded destination and emits a typed `ExplorerEvent::OpenDestination`.
The shell composition root forwards that event to the Workspace, which opens
and activates the Explorer dock. The notification action remains data-only;
the destination path remains in Explorer-owned state keyed by notification ID.

## Consequences

- The Explorer action opens the dock and navigates to its destination.
- Workspace receives a typed view event and owns only dock activation.
- No feature-specific notification routing or destination state is added to
  the shell or Workspace.
