# Settings Contract

**Status:** Normative
**Version:** 2

## What settings are

Settings are typed, persisted values that change application behavior or presentation. They may have global, user, and project/workspace layers when that scope is meaningful.

Settings are a value service, not a feature directory. A setting belongs here
only when changing the value changes behavior or presentation and the owning
feature can consume it through the typed settings contract.

## What settings are not

The settings system is not the owner of:

- saved hosts or credentials;
- theme or icon-theme catalogs;
- keymap definitions and shortcut editing (the Keymap page links to the
  Keymap-owned editor);
- notifications;
- transfer history;
- command registration;
- feature management screens.
- MCP/agent bridge runtime configuration;
- statusbar item placement and panel visibility.

There is no `Shortcuts` or `Icon Themes` settings category. The
`Themes` page only hosts the theme and background owners' surfaces inline
(ADR 0006, below); it owns no catalog or state. The pinned Settings taxonomy has a `Keymap` page that opens the
Keymap owner's canonical editor; it does not add a second binding editor or
move binding state into Settings. The `Hosts` page, directly under `Keymap`,
embeds the Hosts owner's management view as a full-page inline surface
([ADR 0007](adr/0007-hosts-settings-page.md)); Hosts keeps all state and
persistence, and Settings stores no host or credential data. Theme management
remains a separate capability surface.

Those capabilities have their own modules and entry points.

Persisting a value does not transfer ownership to Settings. For example, the
active color-theme ID or icon-theme ID may be stored as a user preference, but
the theme module owns the registry, preview, validation, and selection flow.
Likewise, keymap data is owned by the keymap module and host records by the
hosts module; neither becomes a Settings category merely because it is saved
to disk. MCP preferences and statusbar/panel layout follow the same rule and
are loaded by their owning capabilities.

Workspace dock and sidebar state is persisted by `labonair-workspace` in
`workspace-layout.json`. The legacy `workspace.sidebar*`, `workspace.dockLayout`,
and v1 `preferences` counterparts are migration input only; they are not
editable Settings fields and must not be added to the project-settings
whitelist. Layout migration runs before Settings conversion for v1 input and
also handles existing v2 files. It is idempotent: a valid workspace layout
file is never overwritten.

## Settings pages

The visible Settings navigation is a UI taxonomy, independent of the seven
persisted `SettingsContent` groups. Page descriptors route stable JSON field
paths to sections and search results; they do not rename stored keys or take
ownership away from the capability that consumes each value. A field has one
canonical edit page even when its persisted group supplies values to multiple
pages. Capability-owned links use `SettingsSurfaceRegistry`: each contribution
provides a stable ID, searchable title and description, canonical action label,
and page placement. The registry validates IDs and page metadata, then supplies
the same snapshot to navigation and search. Settings invokes the contribution's
owner action; it does not add a feature-specific renderer branch.

The product Settings taxonomy has fifteen page slots: General, Appearance,
Keymap, Editor, Languages & Tools, Search & Files, Window & Layout, Panels,
Debugger, Terminal, Version Control, Collaboration, AI, Network, and
Developer. A page appears in the runtime navigation only when it has
owner-backed settings or links to its owner's canonical editor when the page
is part of the pinned Settings taxonomy; missing
capabilities remain explicit crosswalk work and must not be represented by
empty or inert placeholder controls.

The current runtime pages are General, Appearance, Themes, Editor, Languages
& Tools, Search & Files, and Terminal, followed by the owner page Keymap
(`labonair_settings::FIRST_OWNER_PAGE_INDEX` marks where owner pages start).
Window & Layout, Version Control, and Network were folded into other pages:
tab placement and Zen mode into Appearance, `restoreWindowState` into General
› Startup, the editor Git values into Editor › Git, Explorer/SCM Git values
into Search & Files › Explorer & Source Control, and the SSH defaults into
Terminal › Connection.

Appearance holds value fields only (colour scheme, Classic/Glassy interface
style, tabs, panes, density, typography, motion, Zen mode). Themes hosts the
Theme owner's colour-theme and icon-theme surfaces and the Background owner's
image editor in their own sections (ADR 0006). A surface registered with
`SettingsSurfaceMode::Inline` (and an inline builder) is embedded by the
Settings window; a `Link` surface shows its action button. The Background
surface is inline and its former options modal is removed; the `open`
callback only opens Settings on the Themes page. Settings never keeps
catalogs, preview state, image state, or persistence for these surfaces.

`Terminal › Connection` holds global SSH connection-behaviour defaults
(handshake timeout, keep-alive interval, keep-alive failure tolerance) — the
fallback a host record falls back to when it doesn't set its own value. It
does not manage hosts, credentials, or the connection list; that remains the
Hosts module's surface.

Update policy is a General field and is grouped under the General page's
Updates section; it is not a separate management category.

Every JSON-backed field row can copy a `labonair://settings/<json-path>` link.
The macOS application receives that URL through GPUI's open-URL callback,
validates the path against the Settings field registry, and opens Settings at
the owning page and field. Unknown paths are ignored.

Host and credential management is the Hosts owner's full-page surface embedded
on the `Hosts` page (ADR 0007), and theme and icon-theme catalogs are
not Settings state, even when selected IDs or defaults are persisted through
the settings storage layer; the Themes page embeds the theme owner's surfaces
inline rather than duplicating them. The Keymap page is a navigation surface for the existing Keymap-owned
editor; keymap editing and persistence remain outside Settings.

The Keymap editor is reachable from both the Settings navigation and the
titlebar global menu; both entry points open the same Workspace-owned Keymap
tab. Themes and Icon Themes open their respective pickers, and Hosts opens
the Settings `Hosts` page. The themes and hosts modules own those flows;
Settings may persist only a typed preference exposed by their contracts.

Categories may be added for a capability-owned editor only when that category
exists in the pinned Settings taxonomy. The page links to the canonical owner
surface and never duplicates its editor, state, or persistence. Other
management workflows remain outside Settings.

Do not add a category for a registry, resource catalog, connection list,
history view, or feature workflow. Those belong to the owning module's
surface. If a category becomes empty, duplicate, or unused, remove it and
migrate only values that still have a consumer.

## Field rules

Every setting has:

- a typed field;
- a stable key;
- a default;
- documented scope;
- validation;
- reset behavior;
- a visible description.

Unknown values should survive migrations when safe. Removed settings require a deliberate migration or an explicit compatibility decision.

Before retaining a field, identify its consumers and scope. A field with no
current consumer is removed or explicitly deprecated with a removal condition;
it is not kept as a placeholder for a possible future feature. Settings
migrations must be idempotent, preserve user data where safe, and never move
ownership of hosts, themes, keymaps, notifications, or transfers into the
settings crate.

## UI rules

Generated field UI is preferred for ordinary values. A custom view is allowed only when the interaction cannot be represented as a field, and it must still use the standard settings chrome and UI-kit components.

The native Settings window has one dense tree rail and one content surface:

- its initial outer bounds are 900×750 logical pixels scaled by
  `ui_font_size / 16`; the minimum is 626×240 logical pixels;
- the rail is 226 logical pixels wide and contains the owner-backed pages
  listed above; persisted `SettingsContent` groups and group headings are not
  a second navigation taxonomy. Each category row shows a coloured
  `category_chip` and, when selected, the sunken fill with a semibold label;
  the owner pages sit below the value categories with a gap (Keymap and, below it,
  Hosts register one; Hosts is a full-page inline surface, ADR 0007);
- the rail uses 10 logical pixels of side and bottom inset and a 40-pixel
  macOS top inset below the native titlebar. Its shared search field is 28
  pixels high with a 12-pixel gap before navigation;
- the rail spans the full settings surface; the page header sits in the right
  content pane: back/forward buttons through the visited pages (also
  `cmd-[` / `cmd-]`), the page title, the scope selector and the JSON action;
- the content surface keeps at least 400 logical pixels for field descriptions
  and controls, with 24 pixels of content inset;
- a page is a list of section cards. Each section is one `settings_group`
  card under a 15 px semibold title and one virtualized list item; its fields
  are `settings_row`s. The leading column wraps the title, description and
  notes (hint, "Modified in ...", project-scope explanation); a modified
  setting shows its reset action beside the control, which stays aligned in the
  trailing column. Hairlines separate rows inside a card; there is no card in a
  card;
- field controls are chosen by `FieldControl`: switch, number field, select,
  font family, text, SFTP columns, `Segmented` (short closed choices such as
  density or tab-bar location), `Slider` (bounded numbers such as font size,
  line height, corner radius and terminal opacity; the focused row also steps
  with the arrow keys) and `Tiles` (radio-style preview tiles for colour
  scheme, interface style and pane layout; `translucentSurfaces` is the
  Classic/Glassy bool). `set_field_value` stays the single write path;
- owner surfaces registered inline (colour themes, icon theme, background) are
  embedded as rows of the section card on the Themes page; link surfaces keep
  their button. Search finds the surfaces and, for inline ones, navigates to
  their section;
- the navigation search is inset below the native titlebar controls, while the
  right content header begins at the top of the content pane;
- the header exposes visible `User` / `Project` segments when a project is
  active and an outlined action opens that scope's JSON file; the scope control
  uses the shared medium button geometry and supports click, Tab, Enter/Space,
  and Left/Right navigation;
  project writes are rejected by the Settings owner unless the key is on
  `PROJECT_SETTINGS_WHITELIST`;
- reset removes the selected override from its sparse JSON layer so the next
  lower-precedence value becomes effective. It must not write a duplicate
  default value;
- search, section navigation, and field pages use bounded or virtualized lists;
  long pages must not eagerly materialize every row;
- ordinary controls are UI-kit controls: shared search-field chrome, the
  standard text-field frame with native text/number editors, select
  triggers/popovers, switches, badges,
  disclosure/tree rows, and keyboard hints. Errors go to the notification
  center rather than a second Settings-only toast/banner system.
- system-font discovery exposes its loading state, reports failures through
  the notification center, and offers an in-place retry action.

The Settings UI follows the Labonair product, design-system, and settings
contracts for layout, navigation, focus, field behavior, and state coverage.
Settings ownership and typed persistence remain canonical. Management pages
require an owning Labonair capability.

## Settings UI Guidelines

**Related:** [`settings.md`](settings.md), [`design-system.md`](design-system.md)

These rules define how the Settings surface may evolve. Settings is a value
editor, not a general application-management window.

1. **One navigation model.** Use category → section → optional detail page.
   Do not add a second sidebar, modal taxonomy, or feature-specific navigation
   inside Settings.
2. **Typed fields first.** Every ordinary setting is represented by a typed
   field with a stable key, default, scope, validation, description, and reset
   behavior.
3. **No management pages.** Themes, icon themes, keymaps, notifications,
   transfers, and command registration are opened through their own canonical
   surfaces; rule 18 allows the Themes page to embed the theme and Background
   owner surfaces inline, and the Hosts page embeds the Hosts owner's
   management view as a full-page inline surface
   ([ADR 0007](adr/0007-hosts-settings-page.md)).
4. **Persisted selections are not ownership.** A selected theme ID or similar
   value may be persisted by the settings storage layer while the owning module
   controls its registry and user flow.
5. **Custom panes are exceptional.** Use a custom pane only when the interaction
   cannot be expressed as fields. It must keep the standard Settings chrome and
   UI-kit components.
6. **No duplicate controls.** A value has one canonical editor. Other surfaces
   may preview or invoke an action but must not create a second settings form.
7. **Notifications are global.** Validation and operational failures that need
   user attention enter the notification registry. Do not add feature-local
   toast or inline error systems.
8. **Scope is explicit.** Document whether a field is global, user, profile,
   project, or workspace scoped. Do not silently write project data into user
   settings.
9. **Removal is complete.** Removing a field requires checking defaults,
   migrations, serialization, UI metadata, command IDs, tests, and documentation.

10. **Dense native navigation.** The Settings rail is one UI-kit tree over the
    owner-backed pages in the [Settings contract](settings.md).
    Root selection and disclosure are separate actions; section children are
    scroll anchors, not management pages. Pages start collapsed; selecting a
    root does not disclose its section anchors. Search results and deep links
    expand the page when they need to reveal a specific section.
11. **Explicit persistence scope.** The header's User/Project selector is the
    only scope choice for a Settings edit. A Project choice is available only
    with an active project; the Settings owner enforces the project whitelist,
    rejects malformed project JSON, and persists sparse overrides.
12. **Native editing and long-list behavior.** Text and numeric values use
    real UI-kit `InputState` editors when activated. Selects support focused
    keyboard opening and arrow/Enter selection. Field content uses GPUI's
    variable-height virtualized list; no page may eagerly build an unbounded
    number of rows.
13. **Native keyboard and accessibility fallback.** Interactive Settings
    controls expose visible focus and keyboard activation/navigation wherever
    GPUI 0.2.2 permits it. That GPUI version does not expose native
    accessibility roles or ARIA-style names/values; do not describe this as
    screen-reader parity. Revisit semantic roles and announcements when the
    native accessibility API becomes available.

14. **Unavailable project values are visibly inert.** In Project scope, fields
    outside the project-settings whitelist keep their displayed value and
    explanatory description, but their control cannot be focused or activated.
    Commit or cancel an active edit against its original scope before switching
    the write target; never defer a predictable whitelist failure until after
    the user attempts to change a value.

15. **Navigation is independent of persistence.** A page descriptor owns its
    stable key, title, slug, field placements, and persisted-group fallbacks.
    Rows and search routes resolve by full JSON field path. One field has one
    canonical Settings editor; explicit cross-page placement wins over a
    persisted-group fallback. New fields remain reachable through their
    owning page's fallback until their deliberate placement is recorded.

16. **Modified values identify their source.** Show the reset action beside the
    setting title and identify whether the effective override comes from User
    or Project settings. Reset clears that layer's sparse override so the next
    lower-precedence value takes effect.

17. **Setting links resolve to fields.** A JSON-backed row may copy a
    `labonair://settings/<json-path>` link. Opening a known link selects the
    owning page, scrolls to the field, and briefly highlights it; an unknown
    path must not open an unrelated Settings page.

18. **Owner surfaces may be embedded inline.** The Themes page hosts the
    colour-theme, icon-theme and Background owner surfaces
    ([ADR 0006](adr/0006-settings-hosts-owner-surfaces-inline.md)). A surface
    registered with `SettingsSurfaceMode::Inline` and an inline builder is
    embedded by the Settings window; the owner keeps all state, persistence and
    notifications. Settings gains no feature state, and no other module may add
    a parallel Settings category. A surface marked `full_page` (Hosts) owns its
    whole page, scrolls itself, and receives the keyboard: typing there is never
    treated as Settings search.

19. **Pages are section cards.** Group values into `settings_group` cards of
    `settings_row`s (title above the card). Pick the control by value shape:
    switch for a bool, segmented control for two to four short choices,
    preview tiles for a choice with a visual identity (colour scheme,
    interface style, pane layout), slider for a bounded continuous number,
    select for longer lists. Do not nest a card inside a card; an inline owner
    view contributes rows to the host card.
20. **Settings navigation has history.** Visiting a page records it; the
    header's back/forward buttons and `cmd-[` / `cmd-]` walk that list.

## Settings and Data Lifecycle

**Related:** [`../settings.md`](settings.md), [`../settings-inventory.md`](settings.md), [`../architecture/runtime-lifecycle.md`](architecture.md)

The lifecycle for a persisted value is:

```text
input → validation → layered in-memory state → persistence → reload
       → migration/unknown-key handling → deletion or retention decision
```

### Ownership

| Data | Owner | Rules |
|---|---|---|
| Settings values and schema | `labonair-settings` / settings content | Values only; typed scopes and defaults. |
| Settings UI/navigation | `labonair-settings-ui` | Renders values and validation; no feature ownership. |
| Host metadata | Hosts | SQLite/domain ownership; no raw secret payload. |
| Credentials/key material | Credentials + Secrets | References and keychain/secret boundary. |
| Workspace/session/layout | Workspace | Versioned snapshots, migration, and recovery. |
| Notifications | Notifications | In-memory retained lifecycle; no settings projection. |
| Themes/keymap/transfers | Their owning modules | No parallel Settings management category. |

### Change requirements

Every new or changed value records:

- stable key and scope (`app`, `workspace`, `host`, or `session`);
- default and validation rule;
- storage file/table and serialization format;
- load/save/reload and malformed-input behavior;
- migration and unknown-key decision;
- live-reload versus restart behavior;
- secret classification and redaction rule;
- owning consumer, UI entry point, tests, and evidence.

Delete dead fields or attach a named removal task. A parser accepting a field
is not evidence that the field still belongs in the product.
