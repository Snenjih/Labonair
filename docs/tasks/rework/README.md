# Open tasks

Open work only. Finished tasks (R00–R08 done, R07-004/005/006, R09-001/002/004/005, R11-001) live in [`../archive/rework-done/`](../archive/rework-done/); everything older is in git history (tag `pre-cleanup-2026-11-21`).
This is a backlog, not a gate: a direct user request always takes precedence. Status below comes from a code/commit audit on 2026-11-21 — "code exists" is not the same as natively verified.

| Task | State | What is left |
|---|---|---|
| R07-001 product-surface acceptance | Partial | Native visual-state matrix (one open box); also covers the visual checks of closed R07-004/005 and R09-001..005 |
| R07-007 transitional-edge review | Open | Review remaining transitional edges in `scripts/crate-graph.toml` |
| R07-008 multi-instance / detached sessions | Partial | Code landed (4cc0d575); acceptance boxes unchecked, needs two-process tests |
| R08-010 actionable notifications | Deferred | No consumer; build only when a feature needs it |
| R09-003 editor E2 editing | Partial | Splits dropped (view groups, ADR 0005); verify command registration/keymap overrides, multi-select on folded views |
| R09-006 editor E5 git gutter/review | Partial | Core + bridge done; verify alignment after reload, notification routing, settings records |
| R09-007 editor E6 persistence/polish | Partial | Persistence, save policies, autosave exist; need failure/conflict tests, P2 deferral list |
| R10-001 Ciel design system | Partial | Light theme + Gallery visual check, view sweep, gates |
| R10-002 UI overhaul | Partial | workspace tests, `check-crate-deps.sh`, `git diff --check` boxes |

New work: copy [`TASK-TEMPLATE.md`](TASK-TEMPLATE.md).
