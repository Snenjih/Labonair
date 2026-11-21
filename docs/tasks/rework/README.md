# Open tasks

Open work only. Finished tasks (R00–R08 done, R11-001) live in [`../archive/rework-done/`](../archive/rework-done/); everything older is in git history (tag `pre-cleanup-2026-11-21`).
This is a backlog, not a gate: a direct user request always takes precedence. Status below comes from a code/commit audit on 2026-11-21 — "code exists" is not the same as natively verified.

| Task | State | What is left |
|---|---|---|
| R07-001 product-surface acceptance | Partial | Native visual-state matrix (one open box); gate for R07-004/005/006/007 |
| R07-004 explorer host contract | Done structurally | Native visual evidence (folds into R07-001) |
| R07-005 background presentation boundary | Done structurally | Native visual evidence (folds into R07-001) |
| R07-006 documentation knowledge system | Obsolete | Superseded by the 2026-11-21 docs consolidation; archive |
| R07-007 transitional-edge review | Open | Review remaining transitional edges in `scripts/crate-graph.toml` |
| R07-008 multi-instance / detached sessions | Partial | Code landed (4cc0d575); all acceptance boxes unchecked, needs two-process tests |
| R08-010 actionable notifications | Deferred | `action.id` support exists, no consumer; reassess |
| R09-001 editor E0 text model | Likely done | Spot-check acceptance list, then close |
| R09-002 editor E1 file lifecycle | Likely done | Spot-check, then close |
| R09-003 editor E2 editing/splits | Partial | Spec outdated: editor view groups replaced internal splits (cbd41bc9) |
| R09-004 editor E3 display/search/nav | Likely done | Spot-check, then close |
| R09-005 editor E4 language services | Likely done | Spot-check, then close |
| R09-006 editor E5 git gutter/review | Partial | Beyond scope in code; reconcile with task text |
| R09-007 editor E6 persistence/polish | Partial | Verify autosave / versioned persistence |
| R10-001 Ciel design system | Partial | Light theme + Gallery visual check, view sweep, gates |
| R10-002 UI overhaul | Partial | workspace tests, `check-crate-deps.sh`, `git diff --check` boxes |

New work: copy [`TASK-TEMPLATE.md`](TASK-TEMPLATE.md).
