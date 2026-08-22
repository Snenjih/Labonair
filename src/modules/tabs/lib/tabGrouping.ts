import type { Tab, WorkspaceTab } from "../types";

// --- Sidebar tab grouping by folder ---

/** The raw path a tab is "in", with no host component — grouping matches
 *  purely on this string, so a local and a remote tab sharing the same
 *  folder land in the same category (see `remoteHostLabelFor` in
 *  `tabUtils.tsx` for the separate per-tab "Remote" badge that disambiguates
 *  them within a group). `undefined` for kinds with no folder concept. */
export function pathKeyFor(t: Tab): string | undefined {
  if (t.kind === "workspace") {
    const wt = t as WorkspaceTab;
    return wt.sessions[wt.activePaneId]?.cwd;
  }
  if (t.kind === "editor") return t.remoteHostId ? (t.remotePath ?? t.path) : t.path;
  if (t.kind === "ai-diff") return t.path;
  if (t.kind === "sftp") return t.remotePath;
  if (t.kind === "git-graph" || t.kind === "commit-diff") return t.repositoryPath;
  if (t.kind === "git-diff") return t.repoRoot;
  return undefined; // "preview" (URL, not a path) and "home" have no folder.
}

/** Compact group header name — last 2 path segments, e.g.
 *  "/home/user/Developer/active/Labonair" → "active/Labonair". Two distinct
 *  full paths can coincidentally produce the same short name (e.g. same
 *  project folder name on two different hosts) — a cosmetic ambiguity in
 *  the header text only; the grouping itself still keys on the full path,
 *  so it's never actually merged. */
export function groupNameFor(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts.length ? parts.slice(-2).join("/") : "/";
}

export type RenderPlanEntry =
  | { kind: "header"; key: string; name: string }
  | { kind: "tab"; tab: Tab; groupKey: string | undefined };

/** Builds the sidebar's render order: tabs sharing a `pathKeys` value with
 *  at least one other tab are clustered into a header + block, emitted at
 *  the position their key first appears in `tabs`' original order (stable,
 *  no separate group-ordering rule). Every other tab renders individually,
 *  in place, with `groupKey: undefined` — including tabs whose path happens
 *  to be unique (below the 2-tab grouping threshold). */
export function buildGroupedRenderPlan(
  tabs: Tab[],
  pathKeys: Map<number, string | undefined>,
): RenderPlanEntry[] {
  const counts = new Map<string, number>();
  for (const t of tabs) {
    const key = pathKeys.get(t.id);
    if (key) counts.set(key, (counts.get(key) ?? 0) + 1);
  }

  const plan: RenderPlanEntry[] = [];
  const emitted = new Set<string>();

  for (const t of tabs) {
    const key = pathKeys.get(t.id);
    const isGrouped = key !== undefined && (counts.get(key) ?? 0) >= 2;

    if (!isGrouped) {
      plan.push({ kind: "tab", tab: t, groupKey: undefined });
      continue;
    }
    if (emitted.has(key)) continue;
    emitted.add(key);
    plan.push({ kind: "header", key, name: groupNameFor(key) });
    for (const member of tabs) {
      if (pathKeys.get(member.id) === key) plan.push({ kind: "tab", tab: member, groupKey: key });
    }
  }

  return plan;
}

/** A drag is only blocked when both tabs belong to an actual rendered group
 *  (`groupKey` set, i.e. threshold already met) and those groups differ —
 *  reordering among ungrouped tabs (the common case: most tabs have a
 *  unique folder) stays unrestricted. */
export function isBlockedCrossGroupDrag(
  fromGroupKey: string | undefined,
  toGroupKey: string | undefined,
): boolean {
  return fromGroupKey !== undefined && toGroupKey !== undefined && fromGroupKey !== toGroupKey;
}
