# Labonair documentation

Authority order when sources disagree: the user's current decision → these documents → accepted ADRs → open tasks ([tasks/](tasks/)) → everything under `archive/`.

| Document | Content |
|---|---|
| [product.md](product.md) | Product identity, non-goals |
| [architecture.md](architecture.md) | Layers, dependency rules, composition root, runtime, repository layout |
| [modules.md](modules.md) | Module/crate rules and the capability ownership matrix |
| [registries.md](registries.md) | Registry contracts (commands, panels, themes, …) |
| [workspace-model.md](workspace-model.md) | Project vs. standalone workspaces, panes, tabs |
| [settings.md](settings.md) | Settings ownership, value model, scope, persistence and UI contract |
| [design-system.md](design-system.md) | UI-kit and visual consistency rules |
| [security.md](security.md) | Threat model, trust boundaries, secrets, AI guards, remote access, detached host |
| [automation.md](automation.md) | MCP bridge: grants, limits, tool catalog |
| [performance.md](performance.md) | Budgets and baselines |
| [testing.md](testing.md) | Visual verification and native capture runbook |
| [release.md](release.md) | Versioning, packaging, signing (conditional), updater, CI/release workflows, rollback, platforms |
| [gotchas.md](gotchas.md) | Non-obvious GPUI / terminal / settings / build API discoveries, grouped by topic |
| [LICENSES.md](LICENSES.md) | Third-party license notes |
| [ideas/](ideas/) | Non-normative exploratory ideas |
| [tasks/](tasks/) | Task queue (`rework/`) and archived historical tasks |
| [adr/](adr/) | Accepted architecture decision records |
| [archive/](archive/) | Superseded ADRs, old roadmap and audits — historical only |

## ADR index

| ADR | Title | Status |
|---|---|---|
| [0001](archive/adr/0001-crate-decomposition.md) | Decompose the `ui` monolith into ~22 crates | Superseded by 0003 (archived) |
| [0002](archive/adr/0002-app-event-bus.md) | Keep the `AppEvent` bus; add `BackendEventBridge` | Superseded by 0003 (archived) |
| [0003](adr/0003-modular-product-architecture.md) | Establish the modular product architecture | Accepted |
| [0004](archive/adr/0004-editor-internal-splits.md) | Editor-internal splits | Superseded by 0005 (archived) |
| [0005](adr/0005-universal-panes.md) | Universal panes: every tab is a pane group | Accepted (item 7 amended by 0007) |
| [0006](adr/0006-settings-hosts-owner-surfaces-inline.md) | Settings may host owner surfaces inline | Accepted |
| [0007](adr/0007-hosts-settings-page.md) | Hosts management is a full-page Settings surface | Accepted |
| [0008](adr/0008-onboarding-module.md) | Onboarding module, shown as a workspace tab | Accepted |
| [0009](adr/0009-detached-session-host.md) | Detached session host | Accepted (multi-client exclusion superseded by 0010) |
| [0010](adr/0010-shared-workspaces-and-multi-client-detached-sessions.md) | Shared Workspaces and multi-client Detached sessions | Accepted; implementation in progress |
| [0011](adr/0011-actionable-notification-presentations.md) | Actionable notification presentations | Accepted |
| [0012](adr/0012-transfer-notifications.md) | Transfer terminal notifications | Accepted |
| [0013](adr/0013-explorer-notification-navigation.md) | Explorer notification navigation | Accepted |
| [0014](adr/0014-native-os-notifications-and-toast-lifetime.md) | Native OS notifications and fixed toast lifetime | Accepted |
| [0015](adr/0015-preview-merged-into-editor.md) | Preview merged into the Editor | Accepted |
