# 0015: Preview merged into the Editor

## Status

Accepted

## Context

Previewing a file was a second capability: a separate `PaneKind::Preview` / `TabKind::Preview` tab backed by `PreviewView` in `labonair-workspace`, plus three Workspace-side "editor preview" bindings (`markdown_previews`, `svg_previews`, `editor_side_previews`, `following_*`) that opened a Preview pane beside an editor and fed it buffer snapshots. Binary files such as PNG opened in the Editor as an error banner ("binary file") and had to be opened separately through Explorer's "Open Preview". The Preview renderer (images, SVG via `resvg`, Markdown, delimited tables, external-open fallback) and its `image`/`resvg` dependencies lived in Workspace, and so did the Markdown parser it alone used. This violates "one product capability, one owning module": file content presentation belongs to the Editor.

## Decision

- The Editor tab owns two content forms, `ViewMode::Source` (text buffer) and `ViewMode::Preview`. There is no separate Preview tab or backend for local files.
- **Crate placement.** `PreviewView` (renderer, zoom/pan/fit, SVG rasterization, table and Markdown rendering) and the Markdown parser move from `labonair-workspace` into `labonair-editor-ui` (`preview.rs`, `markdown.rs`). `image` and `resvg` move with them. New edge `labonair-editor-ui -> labonair-explorer-host`: a dependency-free value crate that already owns `is_previewable`/`PREVIEW_EXTENSIONS`; no cycle is introduced. `scripts/crate-graph.toml` is updated.
- **Classification** is pure logic in `labonair-editor-ui::preview_mode`: `classify(path) -> Option<PreviewKind>` (`Image`, `Svg`, `Markdown`, `Tabular`, `Pdf`, `External`/HTML) and `open_mode(path)`. Binary kinds (`Image`, `Pdf`) open directly in Preview mode and have no Source form; text kinds open in Source and show a toggle in the editor toolbar. Types without a preview show no toggle.
- **Toggle.** One toolbar button and the command `editor::TogglePreview` (`CommandId::EditorTogglePreview`, default `cmd-shift-v` in the Editor context, moved from `markdown::OpenPreview`). The legacy open-preview commands (`markdown::OpenPreview`, following, side, SVG, table) now just switch the active tab to Preview mode. Side-by-side source+preview is dropped; the toggle replaces it.
- **Sync.** In Preview mode the editor feeds the embedded `PreviewView` the current buffer revision (SVG, Markdown, tables) or reloads it from disk after a file reload (images). Zoom, pan and fit commands (`image_viewer::*`) route to the active editor's preview.
- **Routing.** All file opens already reach `EditorView::open_path`, which picks the initial mode. Explorer "Open Preview" (`Workspace::open_preview`) opens the file in the Editor and forces Preview mode. Only URLs still reach the legacy Preview pane.
- **Persisted sessions.** A saved Preview pane for a local file is restored as an Editor pane (`session_v3::migrate_preview_pane`); URL previews stay legacy until removal.
- **Removed in the follow-up change:** `PaneKind::Preview`/`TabKind::Preview`, `PaneState::Preview`/`TabSnapshot::Preview`, `Workspace::previews` and all `*_previews`/`following_*` state, `pane_items/preview.rs`, `views/preview.rs` shim, the `EditorEvent::*PreviewRequested`/`PreviewToSideRequested` variants and `EditorSidePreviewContent`, the New Preview Tab menu/command, and the legacy `EditorOpen*Preview` command ids.

## Consequences

- Binary images no longer show a "binary file" error; they open as previews in the normal editor tab, dedupe by path and use the normal tab/peek behavior.
- `labonair-workspace` loses its `image`/`resvg` dependencies; `labonair-editor-ui` gains them (plus `tracing`).
- The Source/Preview choice is not persisted: reopened tabs use the default mode for their type.
- Raster zoom is bitmap scaling; SVG is rasterized at a bounded size, not re-rendered per zoom level. There is no transparency checkerboard yet.
- Windowed behavior (toolbar toggle, keyboard focus in the preview) is unit-tested only at the model level; it has not been verified in a running window.
