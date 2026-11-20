# T16-005: `labonair-panel` Contracts-Crate

## Status
✅ Done

## Phase
15 — Crate-Zerlegung & Fundament

## Abhängigkeiten
T16-001 (ADR & Ziel-Crate-Graph)

## Ziel
Den kleinen, abhängigkeitsarmen Crate `labonair-panel` anlegen, der **nur die
Contracts** enthält: die Traits `Panel` und `StatusItem` plus die
Registry-Typen `PanelRegistry` / `StatusItemRegistry`. Er wird in dieser Task
**noch nicht genutzt** — er existiert, damit T16-006/008 die Panel-Crates
darauf umstellen können, ohne einen Zyklus Panel ↔ Workspace zu erzeugen.

## Kontext
- Ziel-Architektur: `docs/architecture.md` (T16-001) — `labonair-panel` hängt
  von **keinem** Workspace-Track-Crate ab.
- Heutiger Port: `enum SidebarPanel` in `crates/ui/src/app_shell.rs:74`
  (`Explorer|Snippets|SourceControl|Tabs|Hosts|Ai`) + `render_panel_body`
  `match`; `BarItemId`-Enum + `render_bar_item`-`match` in `app_shell.rs`.

## Anweisungen zur Umsetzung
1. **`crates/panel/` anlegen** (`labonair-panel`, `src/panel.rs` Lib-Root).
   Dependencies **minimal**: `gpui`, `labonair-gpui-ext`. **Nicht**
   `labonair-workspace`, **nicht** `labonair-ui-kit` (falls doch ein Icon-Typ
   gebraucht wird, einen leichten `PanelIcon`-Enum hier definieren, nicht
   `IconName` importieren — oder `labonair-ui-kit` doch zulassen, falls es
   zyklenfrei bleibt; Entscheidung in `docs/architecture.md` nachziehen).
   - `fn persistent_name() -> &'static str` (stabiler Persistenz-Schlüssel)
   - `fn title(&self, cx: &App) -> SharedString`
   - `fn icon(&self) -> PanelIcon` (für den Statusbar-Toggle)
   - `fn position(&self, cx: &App) -> DockPosition`
   - `fn position_is_valid(&self, pos: DockPosition) -> bool`
   - `fn set_position(&mut self, pos: DockPosition, window, cx)`
   - `fn default_size(&self, cx: &App) -> Pixels`
   - `fn min_size(&self) -> Option<Pixels>`
   - Supertraits: `Focusable + Render` (EventEmitter<PanelEvent> optional —
     nur wenn Zoom/Close-Events wirklich gebraucht werden; sonst später).
   - `enum PanelEvent { Activate, Close, ZoomIn, ZoomOut }`.
4. **`trait StatusItem`** definieren (Port `StatusItemView` + `HideStatusItem`):
   - `fn id(&self) -> &'static str`
   - `fn default_side(&self) -> StatusSide` (`enum StatusSide { Left, Right }`)
   - `fn render(&mut self, window, cx) -> AnyElement`
   - `fn hide(&self) -> Option<StatusItemHide>` (Analogon zu `HideStatusItem` —
     ein `Arc<dyn Fn(&mut App)>` oder ein Marker, dass das Item ausblendbar
     ist; die Persistenz-Anbindung folgt in T18-005).
5. **`PanelRegistry`** + **`StatusItemRegistry`** als schlanke Container:
   - `PanelRegistry`: registriert Panel-Konstruktoren
     (`Box<dyn Fn(&mut Window, &mut App) -> AnyPanelHandle>` o.ä.), listet
     registrierte Panels, liefert sie nach `persistent_name`/`DockPosition`.
   - `StatusItemRegistry`: analog für `StatusItem`.
   - Als GPUI-Global oder als Feld in `Workspace` konsumierbar — hier nur der
     Typ + Methoden, die Anbindung macht T17-001/003.
   - Signaturen **jetzt** so festlegen, dass sie in T17 nicht brechen müssen.
7. **Keine Nutzung**: Kein bestehender Code wird umgestellt. `cargo check`
   muss den Crate bauen (ggf. `#[allow(dead_code)]` mit Begründungskommentar,
   bis T17 ihn verdrahtet).

## Akzeptanzkriterien
- [ ] `crates/panel/` ist Workspace-Member; `cargo tree -p labonair-panel`
      zeigt **keine** Kante zu `labonair-workspace`, `labonair-shell` oder
      einem `labonair-panel-*`.
- [ ] `Panel`, `PanelEvent`, `DockPosition`, `StatusItem`, `StatusSide`,
      `PanelRegistry`, `StatusItemRegistry` sind öffentlich + dokumentiert.
- [ ] `cargo doc -p labonair-panel` ohne Warnungen.
- [ ] Gates grün: `cargo fmt --check`, `cargo check --workspace --all-targets`,
      `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace`.

## Notizen
- Bewusst „leerer" Crate. Der Wert entsteht in T17 — aber die Trennung *jetzt*
  verhindert, dass T16-006/008 einen Import-Zyklus bauen, den man später teuer
  auflösen muss.
- `Tabs` ist heute eine `SidebarPanel`-Variante — im neuen Modell ist die
  Tab-Leiste Teil der Titlebar (Layout-Vertrag), **kein** Panel. Nicht in die
  Panel-Liste übernehmen.

## Warnungen
- ⚠️ Kein `serde` in diesem Crate für die Persistenz-Blobs — die
  Persistenz-Struktur (`statusBarItemPlacements`, `DockData`) gehört in
  `labonair-workspace` bzw. `labonair-settings`, nicht in die Contracts.

## Weiterführende Tasks
- [T16-006: `labonair-workspace` extrahieren](./T16-006-extract-workspace-crate.md)
- [T17-001: `Panel`-Trait & `PanelRegistry` verdrahten](../phase-16-registries/T17-001-panel-trait-and-registry.md)
