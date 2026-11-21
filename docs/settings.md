# Settings

Related: [`architecture.md`](architecture.md), [`modules.md`](modules.md) (Settings row), [`design-system.md`](design-system.md).

Settings are typed, persisted values that change application behavior or presentation, layered default < user < project where a project scope is meaningful. Settings are a value service, not a feature directory: a setting belongs here only when its owning feature consumes it through the typed settings contract.

## 1. Ownership

Settings does **not** own saved hosts or credentials, theme/icon-theme catalogs, keymap definitions and shortcut editing, notifications, transfer history, command registration, MCP/agent-bridge runtime configuration, statusbar item placement, panel visibility, or any feature management screen. Persisting a value does not transfer ownership: the active theme ID may be stored as a user preference while the theme module owns registry, preview, validation, and selection; keymap data belongs to the keymap module and host records to the hosts module.

| Data | Owner | Rules |
|---|---|---|
| Settings values and schema | `labonair-settings` / `-settings-content` | Values only; typed scopes and defaults. |
| Settings UI and navigation | `labonair-settings-ui` | Renders values and validation; no feature ownership. |
| Host metadata | Hosts | SQLite/domain ownership; no raw secret payload. |
| Credentials, key material | Credentials + Secrets | References and keychain/secret boundary. |
| Workspace, session, layout | Workspace | Versioned snapshots, migration, recovery. |
| Notifications | Notifications | In-memory retained lifecycle; no settings projection. |
| Themes, keymap, transfers | Their owning modules | No parallel Settings management category. |

Workspace dock/sidebar state is persisted by `labonair-workspace` in `workspace-layout.json`. The legacy `workspace.sidebar*`, `workspace.dockLayout`, and v1 `preferences` counterparts are migration input only: not editable Settings fields and never on the project whitelist. Layout migration runs before Settings conversion for v1 input, also handles existing v2 files, and is idempotent (a valid layout file is never overwritten).

## 2. Value model and persistence

- **Layers.** Default (embedded `crates/settings-content/assets/settings/default.json`, kept in sync with `SettingsContent::defaults()` by a test), user (`~/.config/labonair/config.json`), project (`<root>/.labonair/settings.json`). Each leaf is an `Option<T>` so a layer can distinguish "unset" from an explicit zero value.
- **Content groups.** `SettingsContent` has seven persisted groups: `general`, `appearance`, `terminal`, `editor`, `fileManager`, `workspace`, `connections`. Keys are camelCase. Legacy keys without a field (hosts, mcp, Vim `:set` internals, ...) survive under `_migratedUnknown`; `legacy_migrations` converts `schemaVersion` 1 files once (idempotently), and moves legacy keybinds into `keymap.json`.
- **Project scope.** A project write is accepted only for keys on `PROJECT_SETTINGS_WHITELIST` (`crates/settings/src/project.rs`): `general` startup/restore keys, the `workspace` command-palette keys, and the editor behavior keys (`editor*`, vim cursor shapes). `appearance`, `fileManager`, `terminal`, `connections`, MCP and keymap stay user-only. Malformed project JSON keeps the last good settings.
- **Writes.** UI edits update the in-memory store immediately; surgical JSONC text edits (comments and unknown keys preserved) are serialized on one FIFO writer thread (`labonair-settings-writer`) and flushed at shutdown. File watchers read and parse on the background executor; only applying the parsed layer reaches the GPUI thread. `set_field_value` is the single UI write path.
- **Reset** removes the override from its sparse JSON layer so the next lower-precedence value takes effect; it never writes a duplicate default.

### Field rules

Every field has a typed definition, a stable key, a default, a documented scope, validation, reset behavior, and a visible description. Unknown values survive migrations when safe; removal needs a deliberate migration or compatibility decision. Before retaining a field, identify its consumers; a field with no consumer is removed or deprecated with a named removal condition, not kept as a placeholder. A parser accepting a field is not evidence it still belongs. Removal checks defaults, migrations, serialization, UI metadata, command IDs, tests, and docs. Migrations are idempotent, preserve user data, and never move ownership of hosts, themes, keymaps, notifications, or transfers into the settings crate.

Every new or changed value records: key and scope (`app`, `workspace`, `host`, or `session`); default and validation; storage file/table and format; load/save/reload and malformed-input behavior; migration and unknown-key decision; live-reload versus restart; secret classification and redaction; owning consumer, UI entry point, and tests.

## 3. Pages and navigation

The visible navigation is a UI taxonomy independent of the persisted groups. Page descriptors (`crates/settings-ui/src/pages.rs`) own a stable key, title, slug, field placements, and persisted-group fallbacks; rows and search routes resolve by full JSON field path. A field has one canonical edit page; explicit placement beats a group fallback, and new fields stay reachable through their group's fallback until placed. Sub-pages (e.g. General › Command Palette, Appearance › Pane Focus, Editor › Minimap & Scrollbar, Languages & Tools › Error Lens & Inlay Hints) are scroll-anchored sections, not management pages.

Current pages, in order: **General, Appearance, Themes, Editor, Languages & Tools, Search & Files, Terminal**, then the owner pages **Keymap** and **Hosts** (`labonair_settings::FIRST_OWNER_PAGE_INDEX` = 7 marks where owner pages start). Folded in: tab placement and Zen mode in Appearance; `restoreWindowState` in General › Startup; update policy in General › Updates; editor Git values in Editor › Git; Explorer/SCM Git values in Search & Files › Explorer & Source Control; SSH defaults in Terminal › Connection (handshake timeout, keep-alive interval/tolerance — the fallback for host records; hosts and credentials stay with Hosts).

Target (not yet true): the pinned product taxonomy has fifteen slots (General, Appearance, Keymap, Editor, Languages & Tools, Search & Files, Window & Layout, Panels, Debugger, Terminal, Version Control, Collaboration, AI, Network, Developer). A page appears only when it has owner-backed settings or links to its owner's editor; never add empty or inert placeholder pages. Do not add a category for a registry, catalog, connection list, history view, or workflow; remove empty, duplicate, or unused categories and migrate only values that still have a consumer.

### Owner surfaces

Capability-owned surfaces register through `SettingsSurfaceRegistry` (stable ID, searchable title/description, action label, page placement, `SettingsSurfaceMode`); the registry feeds navigation and search from one snapshot, and Settings invokes the owner action without a feature-specific renderer branch. The owner keeps all state, persistence, and notifications.

- `Link` surfaces show their action button. `Inline` surfaces with an inline builder are embedded; search navigates to their section.
- **Themes** hosts the theme owner's color-theme and icon-theme surfaces and the Background owner's image editor inline as section rows ([ADR 0006](adr/0006-settings-hosts-owner-surfaces-inline.md)); Settings keeps no catalogs, preview, image state, or persistence. There is no `Shortcuts` or `Icon Themes` category.
- **Keymap** links to the Keymap-owned editor (the same Workspace Keymap tab as the titlebar global menu); Settings holds no binding state.
- **Hosts** embeds the Hosts owner's management view as a `full_page` inline surface ([ADR 0007](adr/0007-hosts-settings-page.md)): it owns its whole page, scrolls itself, and receives the keyboard (typing there is never Settings search). Settings stores no host or credential data.
- No other module may add a parallel Settings category.

### Deep links

Every JSON-backed row can copy a `labonair://settings/<json-path>` link. The macOS app receives it through GPUI's open-URL callback, validates the path against the field registry, opens Settings on the owning page, scrolls to the field and briefly highlights it. Unknown or malformed paths (empty, containing `/`, `?`, `#`) are ignored and never open an unrelated page.

## 4. Settings window and controls

- One native window, one dense tree rail plus one content surface. Initial bounds 1120x860 logical pixels scaled by `ui_font_size / 16`; minimum 626x240.
- **Rail:** about 22% of the window width, clamped to 226-320 px, on the sidebar surface with a right border. It has 10 px side/bottom inset and a 40 px top inset below the native titlebar, a shared search field above the navigation, and each category row shows a colored `category_chip` (selected: sunken fill, semibold label). Root selection and disclosure are separate; pages start collapsed, section children are scroll anchors, and search results/deep links expand a page only to reveal a section. Owner pages sit below the value categories with a gap.
- **Content pane:** the header holds back/forward buttons through visited pages (also `cmd-[` / `cmd-]`), the page title, the scope selector, and the JSON action. Content has at least 400 px for descriptions and controls and a 24 px inset.
- **Scope:** visible `User` / `Project` segments (only with an active project) are the only scope choice for an edit; the outlined JSON action opens that scope's file. The control uses shared medium button geometry and supports click, Tab, Enter/Space, Left/Right. In Project scope, fields off the whitelist keep their value and explanatory description but are inert (not focusable or activatable); commit or cancel an active edit against its original scope before switching targets. Never defer a predictable whitelist failure until after a user edit.
- **Modified values** show the reset action beside the title and identify whether the override comes from User or Project.
- **Pages are section cards:** each section is one `settings_group` card (see [`design-system.md`](design-system.md) §Settings widgets) under a 15 px semibold title, and one virtualized list item; fields are `settings_row`s with notes (hint, "Modified in ...", project-scope explanation) in the leading column. No card in a card; inline owner views contribute rows to the host card. Search, section navigation, and field pages use bounded/virtualized lists (GPUI variable-height list); no page eagerly builds an unbounded number of rows.
- **Controls** are chosen by `FieldControl`: `Switch`, `Int`, `Float`, `Select` (longer lists), `FontFamily`, `Text`, `SftpColumns`, `Segmented` (two to four short choices, e.g. density, tab location), `Slider` (bounded numbers: font size, line height, corner radius, terminal opacity; the focused row steps with arrow keys), `Tiles` (radio-style preview tiles for color scheme, interface style, pane layout; `translucentSurfaces` is the Classic/Glassy bool) and `Color`. Generated field UI is preferred; a custom pane is exceptional, only when the interaction cannot be a field, and must use standard Settings chrome and UI-kit components.
- **Editing:** text and numbers use real `InputState` editors when activated; selects open from the keyboard with arrow/Enter selection. Interactive controls expose visible focus and keyboard operation where GPUI 0.2.2 allows; it has no native accessibility roles, so do not claim screen-reader parity.
- **Errors and notices** go to the notification registry, never to a Settings-only toast/banner. System-font discovery shows its loading state, reports failures as a notification, and offers an in-place retry.
- **No duplicate controls:** a value has one canonical editor; other surfaces may preview or invoke an action but not build a second form. Document scope (global, user, profile, project, workspace) and never silently write project data into user settings.
