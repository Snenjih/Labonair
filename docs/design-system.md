# Design System

Related: [`architecture.md`](architecture.md), [`modules.md`](modules.md), [`settings.md`](settings.md).

Labonair adopts **Ciel's design system** (clean-room: measured values and rules, no source copied): calm, dense, precise, fast. Surfaces separate by **fill contrast**, not borders and shadows, except in the comfortable pane layout, where the [floating layer](#floating-layer) lifts panes and docks off a gradient canvas. Labonair owns its implementation, tokens, and identity.

## Core rules

- **Two surface levels.** Chrome (`surface.toolbar/title_bar/status_bar`, `sidebar.background`) holds titlebar, statusbar, and docks; the main area is `core.background`. Inside it, separate by spacing and at most a `core.card`/`core.muted` fill; never border *and* shadow together, never a card in a card.
- **Elevation comes only from shadow tiers.** Menus, dropdowns, popovers, tooltips: 1 px `border.variant` hairline + `popover` tier. Dialogs and the command palette: 1 px `border.strong` hairline + `modal` tier. Panes and docks (comfortable layout): `pane` / `pane_focused`. No view invents a shadow.
- **One active state.** Selected/active rows, tabs, nav items, and toggles use `surface.sunken` plus a semibold label; hover is `core.accent`. No accent tint, side stripe, or underline. The blue `core.primary` is for links, toggled markers, and genuine "changed" highlights.
- **The primary action is neutral.** `ButtonVariant::Action` uses `surface.action` (dark in light themes, light in dark), once per view.
- **Every button has chrome** (fill or outline) except breadcrumb-style navigation text; icon-only buttons may be ghost.

## Token ownership

All colors, spacing, radii, typography, shadows, focus states, and semantic status colors come from the theme/design-token layer (`labonair-theme-tokens`, `labonair-theme`; the ui-kit reads them through `Palette`). Feature crates consume semantic roles (surface, text, focus, selected, danger, spacing), never a feature-specific value such as `host-row-blue`. A view may not introduce a global color, spacing, radius, typography, shadow, or state value inline; if a new value is genuinely needed, extend the token source first and record the reason in the owning task. Color-theme and icon-theme registries supply the active values; feature views do not select or persist themes.

### Ciel token map

| Ciel token | Labonair token |
|---|---|
| `--surface` | `core.background`, `core.popover` |
| `--surface-chrome` | `surface.toolbar/title_bar/status_bar`, `sidebar.background` |
| `--surface-muted` | `core.card`, `core.muted`, `core.secondary` |
| `--surface-hover` | `core.accent` |
| `--surface-hover-strong` | `surface.hover_strong` |
| `--surface-sunken` (selected) | `surface.sunken` |
| `--line`, `--line-strong`, `--line-subtle` | `core.border`, `border.strong`, `border.subtle` |
| `--input` / `--field-bg` / `--field-ring` | `core.input` / `Palette::field_bg` / `border.focused` |
| `--accent` | `core.primary` |
| `--action-surface` family | `surface.action`, `action_foreground`, `action_hover`, `action_active` |
| `--radius-sm/md/lg/xl/3xl/bubble/modal` | `RadiusScale` `sm/md/lg/xl/xl2/xl3/xl4` = 4/8/12/14/20/22/30 (window radius fixed 12) |
| `--dur-fast/base/slow` | `Animation` 120/160/200 ms |
| canvas gradient, cards, tab track, hover/pressed | `float.*` (`FloatColors`) |
| `--shadow-*` | `Shadows` tiers `row/pane/pane_focused/popover/modal` |

Built-in themes are `Theme::light()` and `Theme::dark()`; the embedded `labonair.json` is regenerated from them (`REGEN_BUILTIN_THEME=1 cargo test -p labonair-theme builtin_json`). Default colors: light foreground `#1d1d1f`, muted `#636366`, primary `#007cf7`; dark foreground `#f2f2f7`, muted `#aeaeb2`, paper `#1e1e22`. Light canvas `#f6e7f6 → #efe1e6 → #eee0d3 → #ece1bd`, dark canvas `#2c2131 → #231e27 → #221f22 → #2a2619`. Borders are alpha overlays (light black 5-12 %, dark white 4-12 %). The terminal uses soft ink on paper (`#2a2d31` on `#fbfbfb`, `#e6e6ea` on `#1e1e22`) with a softened 16-color palette.

### Typography

- UI chrome: `Inter Variable`, 13 px, line-height 1.5.
- Terminal: `JetBrains Mono` with `SFMono-Regular`, `Menlo`, `monospace` fallbacks; 14 px, line-height 1.05, letter-spacing 0. Weight is a setting (Normal, Medium, Bold).
- Editor: same monospace stack, 13 px, line-height 1.55.
- Text smoothing is the `appearance.fontSmoothing` setting (`system | none | light | medium | strong`), applied app-wide by `labonair-theme-ui`.

## Translucent material

By default every surface is opaque (`WindowBackgroundAppearance::Opaque`). `appearance.translucentSurfaces` (Settings › Appearance › Interface Style, Classic/Glassy tiles, default Classic) switches the main and Settings windows to GPUI's blurred material and paints surfaces with `labonair-theme-tokens::VibrancyLayer` opacities; it applies live to every open window (`labonair_theme_ui::window_background`). `appearance.glassTransparency` (0-100, default 30) scales how far Glassy fills sit below opaque (the table below is the default level; nothing drops under 0.2, dividers never change) and `appearance.glassBlur` (0-100, default 40, macOS) sets the blur strength.

| Surface role | Glassy | Image + Classic | Image + Glassy | Use |
|---|---:|---:|---:|---|
| Canvas | 0.92 | 0.78 | 0.60 | Workspace, editor, Settings content background |
| Toolbar | 0.82 | 0.78 | 0.60 | Titlebar and statusbar chrome |
| Sidebar | 0.86 | 0.78 | 0.60 | Navigation rails and docks |
| Terminal | 0.92 | 0.74 | 0.55 | Terminal surface fill (its own opacity setting still applies) |
| Card | 0.94 | 0.86 | 0.74 | Raised panels and contained surfaces |
| Control | 0.94 | 0.88 | 0.80 | Input surfaces |
| Popover | 0.97 | 0.97 | 0.94 | Menus, dropdowns, floating cards |
| Divider | 0.56 | 0.56 | 0.50 | Structural borders and hairlines |

While a background image covers the window the shell sets `ThemeMetrics::background_active`, and `SurfaceMaterial` (`Opaque`, `Glassy`, `ImageClassic`, `ImageGlassy`) selects the column. The image layer is the first child of the shell root, beneath all chrome. Image opacity is the slider mapped through `slider^0.6` (default 30 % renders about 0.49); tint opacity is clamped when the tint has too little contrast against the UI foreground. Popovers stay near opaque so menus read on any image.

`VibrancyLayer::resolve(color, material)` is the only entry point (a `bool` converts to `Opaque`/`Glassy`); with an opaque material it returns the theme color untouched. Read the material through `UiTheme::surface_material()` / `ThemeStore::surface_material()`, never from `translucent_surfaces` alone. Keep the `gpui-component::Root` clear only while Glassy; Classic (with or without image) paints `core.background` there, under the image layer. Surfaces with their own opaque renderer (terminal cell backgrounds) must consult `ThemeStore::terminal_background()` or the material to stay translucent while an image shows.

In the comfortable layout the canvas and every card on it are two stacked translucent layers; each uses `VibrancyLayer::stacked_opacity_in` / `resolve_stacked` (`1 - (1 - a)^2` equals the role's single-layer opacity), so a card over the canvas is as glassy as a flat Compact surface. Using the full opacity on both compounds to near-opaque and hides the glass.

## Floating layer

The comfortable pane layout (`appearance.paneLayout = comfortable`, default) paints the window as a **canvas with cards on it**; `compact` keeps flat, opaque chrome. Five tokenized parts:

1. **Canvas.** `ui-kit::canvas_backdrop` is the first child of the shell root: a vertical gradient `float.canvas_top` → `canvas_mid` (first 38 %) → `canvas_low` (next 32 %) → `canvas_bottom` (last 30 %), plus a horizontal `float.canvas_tint` wash over the right 65 %. In Glassy/image materials every stop's alpha scales with the `Canvas` opacity.
2. **Cards.** A pane (`pane_frame`) and a dock are cards: `radius.lg` corners, a 1 px rim (`float.pane_border`, `pane_border_focused` when focused), the `float.pane` / `pane_focused` fill (docks keep `sidebar`), and the `pane` / `pane_focused` shadow. A single pane is a card too (no bar). Cards sit `PANE_GAP` = 8 px apart and 8 px from the window edge (a dock has the margin on its outer sides; the pane area supplies the inner one). Card fills are opaque by default because editor/terminal bodies paint their own opaque surface; lowering the alpha shows that body as a solid inset.
3. **Transparent chrome.** Titlebar and statusbar draw no fill or divider, so the canvas shows through. Titlebar tabs sit in a pill track; the selected tab is a `float.tab_active` pill with the `row` shadow.
4. **Neutral state overlays.** Borderless controls hover to `float.hover` and press to `float.pressed` (alpha black on light, white on dark). Icon-only buttons are soft squares (`edge / 2.6` radius).
5. **Focus is depth.** The focused card gets the stronger shadow and rim; the unfocused bar keeps its title at 72 % opacity. Dividers show a 3x36 px grip pill (`muted` at 60 %) while hovered instead of tinting the gap.

### Shadow tiers

All shadows are `Theme::shadows` tiers, read through `Palette` helpers (`row_shadow`, `pane_shadow(focused)`, `popover_shadow`, `modal_shadow`). Each tier is a tight contact layer plus a wide soft ambient layer; a card's ambient layer uses negative spread so it never leaks past the edge. Light themes tint toward the canvas (`60, 40, 50`); dark themes use black.

| Tier | Layers (`x y blur spread`) | Alpha light / dark | Used by |
|---|---|---:|---|
| `row` | `0 1 2 0`, `0 2 8 0` | .07 / .25 | selected tab pill, sticky diff header |
| `pane` | `0 1 2 0`, `0 6 22 -2` | .07 / .25 | unfocused pane, dock |
| `pane_focused` | same geometry | .12 / .40 | focused pane |
| `popover` | `0 1 2 0`, `0 8 24 0` | .12 / .40 | menus, dropdowns, tooltips, toasts, editor popovers |
| `modal` | `0 2 6 0`, `0 18 48 -4` | .12 / .40 | dialogs, command palette |

### Theming the floating layer

Every value is a token a theme can replace.

- **Colors:** a variant's `colors` map gains `canvas.top|mid|low|bottom|tint`, `pane.background`, `pane.background_focused`, `pane.border`, `pane.border_focused`, `tabs.track`, `tabs.track_border`, `tabs.active`, `tabs.separator`, `state.hover`, `state.pressed` (see `COLOR_TOKENS`). Omitted tokens inherit the built-in default of the same appearance.
- **Shadows:** a `shadows` map from tier name (`row`, `pane`, `pane_focused`, `popover`, `modal`) to up to three `{ "x", "y", "blur", "spread", "color" }` layers. Omitted tier inherits; an empty list removes the shadow; unknown tiers and bad colors are skipped with a warning.
- **Flat look:** set the four canvas stops to one color, `canvas.tint` transparent, and every shadow tier to `[]`; `paneLayout = compact` switches the layer off entirely.

```json
{ "name": "Paper", "themes": [
  { "name": "Paper Light", "appearance": "light",
    "colors": { "canvas.top": "#f4f1ea", "canvas.mid": "#f4f1ea",
                "canvas.low": "#f4f1ea", "canvas.bottom": "#f4f1ea",
                "canvas.tint": "#00000000" },
    "shadows": { "pane": [{ "y": 1, "blur": 3, "color": "#00000014" }] } },
  { "name": "Paper Dark", "appearance": "dark" } ] }
```

Known limits: GPUI has no CSS transitions, so hover and focus switch instantly; the separator between two inactive tabs (`float.tab_separator`) is defined but not drawn.

## UI-kit requirement

These must come from `labonair-ui-kit`: buttons and icon buttons; text, search, and multiline text-area fields; search clear actions; lists and list rows; dropdowns, menus, context menus; popovers and dialogs; tabs; continuous sliders; badges, indicators, disclosure rows, tooltips, keyboard hints; dividers, scroll containers, and empty/loading states. Feature modules compose these and provide domain content; they may not create local variants with different padding, colors, hover, or typography without extending the shared component. Exceptions need a documented reason, a composition of existing tokens, an owner, and a follow-up decision on whether the pattern belongs in the kit; one-off local styling is not an exception.

**Dialogs.** Centered modals use `modal_overlay`; owner-rendered contents use `dialog_surface` (fixed scrim, centered placement, card colors, border, radius, spacing, shadow). Owners keep content, focus entry/return, dismissal, validation, and action order. A dialog beside a trigger or at a viewport edge uses `dialog_surface` inside its owner-provided positioner, not another card style.

### Buttons

- **Sizes** (logical px at the reference font size; scale with UI font and density): `None` 16 (inline), `Xxs` 20 (dense status bar), `Xs` 24, `Default` 28 (minimum click target), `Sm` 32, `Lg` 36 (standard form button). Labels: 11 px (`Xxs`), 12 (`Xs`), 13 (`Default`/`Sm`), 14 (`Lg`), medium weight. Corners use the `md` radius (8). Controls in one row share one height.
- **Variants:** default is `Subtle` (ghost); `Filled`, `Outlined`, `OutlinedGhost`, `TintedError` (destructive), neutral `Action` primary, `Link`, `Transparent` (muted text actions), and `Info` (muted readout with neutral hover; pair with `Xxs` in the status bar) are explicit choices. Do not restyle a kit button's colors, radius, or hover at the call site; pick a variant.
- **States:** enabled controls define hover, pressed, and visible focus. Disabled controls keep their geometry, lose their Tab stop, and use the disabled foreground. Render an unavailable action with `button_disabled` (no hover paint, pointer affordance, or handler).
- **One family:** `button` for text and `icon_button_builder` for anything with a glyph; there is no separate toggle (use `.selected(bool)`). The builder supports wide and square geometry, an optional `.label(..)` (always wide), caller-owned selected/disabled state, an alternate selected glyph, a status indicator (optionally bordered to stay distinct), opacity, an independently sized glyph (still font/density scaled), and a tooltip; icon-only actions need a short tooltip label.
- **Zones pick one size:** title bar and tab strip 28 (`Icon`/`Default`), panel headers 24 (`IconXs`/`Xs`), status bar 20 (`IconXxs`/`Xxs`). Ghost icon-only buttons rest in the muted foreground and use the foreground when selected; hover is always the `accent` fill.
- `crates/shell/tests/shared_controls.rs` fails when a file outside the kit adds its own `.hover(..)` style; list rows, resize grips, and scrollbar thumbs are the reviewed exceptions.

### Fields, selects, menus

- Text-field frames: compact 28 px and standard 36 px, `md` radius, `input` border, `field_bg` fill, no hover chrome, `field_ring` border on focus; select triggers 32 px; standard settings text inputs 256 px minimum width. The kit owns padding, border, radius, fill, and focus.
- Menus: 28 px rows, `lg` radius, hairline, popover shadow; group labels uppercase 12 px semibold. Geometry scales with UI font size and density (`Palette::control_space`).
- Select, text-field surface, checkbox, segmented control, and numeric field each have real disabled variants (same geometry, no activation or focus).
- Feature-owned editable text uses the shared `InputState` inside a kit field frame; the owner keeps value, validation, and commit; view-level key handlers do not emulate text editing.
- Select popovers use a caller-owned GPUI `ListState` with virtual rows, height follows row count capped at 320 px. While open: arrows, Home/End, Page Up/Down, Enter/Space choose, Escape dismisses, and the highlighted row scrolls into view. Anchored rich popovers and menu cards call the owner's dismissal on Escape while they hold focus and stop propagation. The Settings select stays controlled by the Settings view.
- Continuous ranges use the shared `Slider`; owners keep `SliderState`, persistence, units, and updates. Pair dragging with a keyboard-operable numeric control when precision matters. Settings `NumberField` is a compact minus / editable value / plus control committing only validated, clamped values (native `InputState`); Appearance ranges use `slider_control`.
- **Positioning.** All anchored menus and popovers use the shared kit positioner: it measures the panel, flips each axis toward the side with more room, clamps inside an 8 px viewport inset, and limits the panel to the usable viewport. Owners supply anchor bounds (same window coordinate space) and content; never a fixed opposite-side location or private placement math.

### Tabs

Workspace and docked tab rows use `tab_item` (horizontal/vertical geometry; selected, dirty, busy, peek, focused, disabled states; shared close button; Enter/Space and orientation-aware arrow activation). Workspace keeps ordering, drag/drop, persistence, rename, and close decisions. The kit's `Classic` style (the `TabStyle` default; the workspace renders [floating tabs](#floating-tabs) instead) is a flat 40 px row with content-sized widths, toolbar-surface bar and inactive tabs, an application-background active tab joining the workspace, hover-only horizontal close, and full-row selected fill with always-visible close for vertical tabs.

## Motion

`ui-kit/src/animation.rs` is the one motion engine. `Palette::motion` (`MotionPrefs`) carries the theme durations (`dur_fast` 120, `dur_base` 160, `dur_slow` 200 ms), the `ease_premium`/`ease_soft` curves, and a `MotionLevel`:

| Level | Behaviour |
|---|---|
| `Full` | fades, slides, size reveals |
| `Reduced` | fades only; offsets and reveals snap to the end state |
| `Off` | every animation collapses to a 10 microsecond floor |

GPUI divides by the duration every frame, so no duration is zero: all go through `MotionPrefs::resolve`. `animation::motion_level` maps `ThemeMetrics::animations` (`appearance.animations`) to the level. `system` is meant to follow the OS Reduce-motion preference but currently resolves to `Full` (`labonair-theme-ui` `resolve_animations`; no reachable platform API without new FFI).

GPUI 0.2.2 has no transforms or hover transitions, so the engine animates only opacity and layout; **hover stays instant on purpose**. `MotionExt` methods: `motion_fade_in`, `motion_slide_in_y` (a `top` lerp), `motion_reveal` (clipped width/height lerp), `motion_state_fade`, `motion_state`.

State transitions use `StateTransition` (previous state in element state): a layer mounting in its end state (startup, scrolling) does not animate, a flip fades, a flip mid-fade continues from the current amount. Build such layers unconditionally (never `when(on, ..)`), as the first child of a positioned parent. `motion_state(.., geometry = true, ..)` lerps a layout property (switch thumb `left`); `Reduced`/`Off` snap it, and `Off` requests no frames. Elements without a `Palette` use `state_fade_ambient`.

Applied by the kit: checkbox fill/tick, `switch` (track fade, thumb slide), segmented active pill, tab selected fill and ring, `ListItem`/`TreeRow` selection fill (hover stays under the fading fill), `disclosure` chevron crossfade (no rotation), and the text-field focus ring when the owner passes `TextFieldState::Focused`. Popovers, menus, and select lists rise 4 px (`POPUP_SLIDE`, `Full` only) while fading in; dialog scrims fade. The tree-row chevron and plain `modal_overlay` do not animate (use `modal_overlay_with_exit`).

### Exit state

Popovers, context menus, popover menus, select lists, and dialogs fade out before unmounting. The owner keeps a `Presence` (open/closing/closed) beside its open state: every dismissal path calls `request_close(now, &c.motion)`, render calls `tick(now, &c.motion)` once, the surface is built while `is_mounted()` using the `*_with_exit` variant with `closing = is_closing()`, and focus returns only when `tick` reports `PresenceStep::JustClosed`. The exit fade is `dur_fast`; repeated close requests do not restart it, reopening cancels it, `Off` closes at once, and a closing surface swallows pointer input. The plain functions (`popover`, `context_menu`, `popover_menu`, `select_popover`, `modal_overlay`) unmount instantly. For owners outside the kit, replace the `Option`/`bool` open flag with a `Presence` (`PresenceSlot` holds the data plus its `Presence`) and route Esc, outside click, and item click through `request_close`. Dock and sidebar reveal use `motion_reveal(id, c, Axis::Horizontal, from, to)` keyed by the open flag, applied only on a flip; tab-in fade uses `motion_fade_in` keyed by tab id.

## Pane chrome

`pane_bar` and `pane_frame` (`ui-kit/src/pane_bar.rs`) are generic (no tab or pane-kind knowledge). The bar is 32 px (`Density::pane_bar_height`) with icon, title, optional leading slot (status marker) and trailing slot (path, cwd), and four optional actions: split horizontal (new pane right), split vertical (new pane below), zoom (toggle), close, each with tooltip, enabled flag, callback. `can_split(false)` disables both splits. The whole bar is the drag handle (`render()` returns `Stateful<Div>`; the caller attaches `on_drag`; action buttons swallow mouse-down); double-click calls the zoom hook.

Comfortable: no divider, no fill, semibold title (72 % opacity unfocused), actions only on the focused bar and on hover (they keep layout space). Compact: `line_subtle` hairline, sunken fill while active, hover accent; no gap, rounding, or ring, and dividers are the tiling container's resize handles. The workspace shows a bar only from two panes on. Because GPUI clips to rectangles, the comfortable pane body is inset by `radius.lg * 0.3`; divider hit areas are 6 px wider than the gap. `PaneLayoutStyle` mirrors `appearance.paneLayout`.

## Settings widgets

`settings_group` is a `radius.lg` card on the `card` fill with `line_subtle` hairlines between rows and an optional caption. `settings_row` puts title and description left and a control right with an optional reset slot (minimum height 44 px, scaled by `control_space`); `SettingsRow::extra` adds notes under the description. Rows use 16 px top/bottom padding, the last row in a section 40 px below, and dividers only between rows of one section. `slider_row` pairs the shared slider with a fixed-width read-out; `slider_control` is the control half with a `disabled` switch for project scope. Inline owner views embedded in a card are row stacks with `line_subtle` hairlines and draw no card. `category_chip` is the 22 px rounded colored badge before a category (fill from a `ChipHue` and light/dark mode, at least 3:1 contrast with its white glyph). The Settings sidebar uses `ListItem` rows with a chip; sections anchors are indented muted rows. Settings composes `TreeRow`, `NumberField`, search-field chrome, `text_field_surface`, buttons, segmented controls, select, `field_input`, badges, and `keybinding_hint`.

`preview_tile_group` renders radio-style tiles with a mini-window mock from theme colors (`TileMock`): color scheme (auto: half light, half dark), interface style, pane layout, theme. The selected tile has a constant 2 px `primary` ring (no layout shift) and a bold label; arrows move and select (wrapping), Enter/Space activate, the owner keeps the value.

## Floating tabs

`TabStyle::Floating` renders 28 px blocks with a constant 1 px border and hover-only close.

- **Horizontal (titlebar):** pills in a 34 px track (`floating_tab_strip`: 2 px padding, 1 px rim, 2 px gaps) filled `float.track`, ringed `float.track_border`. Selected = `float.tab_active` + `row` shadow, faded with the state tween; unselected hover to `float.hover`.
- **Vertical (Tabs panel):** `radius.md` blocks; selected = sunken fill, 1 px `line_subtle` ring, semibold label (the ring is nearly invisible against the sunken fill, so fill and weight carry selection); hover = accent. Column with 6 px padding, 2 px gaps.
- `tab_group_gap` (8 px) replaces the pinned/unpinned divider; `tab_drop_marker` is an absolutely positioned 2 px `primary` insertion bar (no layout shift).

The workspace renders every tab this way (`workspace/src/tab_strip.rs`), reading label, tooltip, kind icon, and status markers only from `tab_display`. The strip is shrinkable and scrolls horizontally; the selected tab scrolls into view once per selection change. Long titles truncate with a full-title tooltip; dirty dot and status markers follow the title. The drop indicator is `tab_drop_marker` on the hovered block, on the side the reorder lands (`drop_side` mirrors `TabStore::reorder`), hidden for rejected drops (pinned onto unpinned, other Space). Strip menus drop from `Density::titlebar_height()`. New blocks fade in with `motion_fade_in` keyed by tab id.

A pane-group tab names itself from its panes: the user's rename wins, else "<active pane title> +N"; same-kind panes show the kind icon, mixed kinds the generic Panes icon, a dirty editor marks the tab. Hovering such a tab (strip or sidebar, active or not) opens after 350 ms a `popover_with_exit` list of its panes (`ListItem`: icon, title, check on active; click focuses). It never opens over a context menu, another popover, a rename, or a drag, and replaces the tooltip.

**Tab tiles** (`ui-kit/src/tab_tile.rs`). A tab shows one 18 px tile per pane: the focused pane in front, up to two more peeking behind (5 px step, 0.8 px height inset, shadow and rim); more than three panes show three. A tile is, in order: the detected coding agent (Claude, Codex, Pi, OpenCode; status badge after the title), an SSH host (SSH tile with the host's picked glyph), the foreground program of a local terminal (`workspace/src/program_tile.rs`: node, python, git/tig/lazygit, vim/nvim/hx/emacs, htop/btop, docker, npm/pnpm/yarn, go/cargo, psql/mysql ...; an unknown program gets a stable hashed color with the terminal glyph; an idle shell the green terminal tile), else the pane kind. Strip, Tabs sidebar, Spaces popover, and pane hover list use the same tiles; the pane header glyph follows the program/agent/host. Own glyphs (`activity`, `cpu`, `hexagon`, `container`, `laptop`, `monitor`, `mac_studio`, `snake_blocks`) are hand-drawn in the project outline style (24 px grid, `currentColor`); third-party brand logos are not used (abstract glyph on a related color).

**Drag and drop.** A dragged tab or pane shows `PaneDragChip` (icon and title on a `popover` surface, 12 px from the pointer). Over a pane area the drop zone is a `primary` highlight (18 % fill, 1 px 70 % ring) over the half the pane would occupy (whole pane for a swap), absolutely positioned and faded with `motion_fade_in`. A pane over a tab block highlights the block (it joins the group); over empty strip space it shows `tab_drop_marker` after the last tab of its Space (a new tab). Rejected targets (other Space, a tab onto itself, pinned merges, zoomed tab) show nothing. Esc cancels; strips scroll near their ends.

## Spaces popover

`render_spaces_popover` (`workspace/src/spaces_ui.rs`) is a 300 px `popover_with_exit`: a `list_header` "Spaces" row with a plus button above a `line_subtle` hairline; each Space is a 28 px `ListItem` (collapse chevron, name or inline rename field, muted tab count), the active Space selected with a semibold label. There is no color badge or `SpaceColor` (old session files with `color` load and drop it on the next write). Tab rows are indented `ListItem`s with a 14 px muted kind icon (active tab in foreground). Recent Projects is a collapsible section with indented folder rows and a hover-only remove button; the pinned footer holds "New space" and "Open Local Folder". Each Space has one hover-only "..." button opening a `context_menu_with_exit` (New Tab, Rename, Close Space; the Default Space cannot be closed). While that menu is up an outside click does not dismiss the popover, so Rename can reach the inline field; dismissing the popover during a rename commits the name, Esc in the field cancels only the rename. The sidebar uses the same header rows.

Workspace-owned surfaces (Spaces popover, per-Space menu, tab context menu, new-tab menu, sidebar new-space dropdown) each live in a `PresenceSlot`: every dismissal calls `close`, `tick` runs once at the top of `Workspace::render`, and focus returns to the active pane only when a surface just unmounted and no rename or other menu is up. The explorer delete confirmation and SFTP permissions/properties dialogs use the same recipe with `modal_overlay_with_exit`; the snippets modal keeps `modal_overlay` (its open state spans several fields). Docks reveal on a flip of their open flag (`workspace/src/dock_reveal.rs`): open grows with `motion_reveal`, close stays mounted for `dur_slow` while shrinking and fading; startup, a zoomed dock, and `Off` snap.

## Interaction states, layout, review

- Every interactive component defines normal, hover, pressed, selected, focused, disabled, and error states where applicable; keyboard focus stays visibly distinguishable. GPUI 0.2.2 exposes neither native accessibility roles (ARIA-style name/value/expanded) nor keyboard-versus-pointer focus modality, so shared controls keep one visible focus treatment for both and rely on visible focus, tooltips, and keyboard behavior. This is a documented limitation, not screen-reader parity; reassess when GPUI exposes a semantics API.
- Use a consistent spacing scale; keep permanent chrome minimal; use progressive disclosure for secondary actions. Lists define selection, focus, empty, loading, and error behavior; long lists use a bounded, scrollable, or virtualized container.
- User-facing error explanations belong in the notification dropdown. A local error state may block an invalid action or mark an invalid control but must not add a second inline message for the same failure.
- **Surface ownership:** feature views own composition and domain content; the kit owns interaction mechanics and visual variants; the shell owns only placement of permanent zones and overlay anchors.
- **Review rule:** a UI change is incomplete until checked at normal, narrow, focused, empty, loading, and error states. Screenshots or a visual comparison are required for shell and shared-component changes (see [`testing.md`](testing.md)).
