// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Editor pane adapter. Panes of one document share a single `EditorView`
//! (shared buffer, history and dirty state); each pane shows its own view
//! group (selection, scroll, folds). The pane whose group is the editor's
//! focused one hosts the interactive text area, the others a mirror.

use gpui::{AnyElement, App, Context, IntoElement, SharedString, Styled, Window};

use super::{EditorBinding, PaneSpec, SplitCloneError};
use crate::pane::PaneId;
use crate::Workspace;

impl Workspace {
    pub(super) fn editor_pane_spec(&self, pane: PaneId, cx: &App, spec: &mut PaneSpec) {
        let Some((editor, _)) = self.editor_binding(pane, cx) else {
            return;
        };
        let editor = editor.read(cx);
        spec.dirty = editor.is_dirty();
        match editor.path() {
            Some(path) => {
                spec.title = SharedString::from(
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Editor")
                        .to_string(),
                );
                spec.trailing = Some(SharedString::from(path.to_string_lossy().into_owned()));
            }
            None => spec.title = SharedString::from("Untitled"),
        }
    }

    pub(super) fn editor_pane_body(
        &mut self,
        pane: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((editor, group)) = self.editor_binding(pane, cx) else {
            return gpui::div().size_full().into_any_element();
        };
        if editor.read(cx).active_group() == group {
            editor.into_any_element()
        } else {
            editor.update(cx, |e, cx| e.mirror_element(group, window, cx))
        }
    }

    /// A new pane on the same document with its own view group.
    pub(super) fn clone_editor_pane(
        &mut self,
        source: PaneId,
        new_id: PaneId,
        cx: &mut Context<Self>,
    ) -> Result<(), SplitCloneError> {
        let unavailable = || SplitCloneError::Failed("The editor is not available.".to_string());
        let slot = self.editor_slot_of_pane(source).ok_or_else(unavailable)?;
        let editor = self.editors.get(&slot).cloned().ok_or_else(unavailable)?;
        let group = editor.update(cx, |e, cx| e.add_group(cx));
        self.editor_aliases
            .insert(new_id, EditorBinding { slot, group });
        Ok(())
    }

    /// Session restore: a second pane of an already restored document.
    pub(crate) fn clone_editor_pane_for_restore(
        &mut self,
        owner: PaneId,
        new_id: PaneId,
        cx: &mut Context<Self>,
    ) -> bool {
        self.clone_editor_pane(owner, new_id, cx).is_ok()
    }

    /// Drop a pane's view group; the document goes with its last pane.
    pub(super) fn retire_editor_pane(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if let Some(binding) = self.editor_aliases.remove(&pane) {
            if let Some(editor) = self.editors.get(&binding.slot).cloned() {
                editor.update(cx, |e, cx| {
                    e.remove_group(binding.group, cx);
                });
            }
            let aliased = self.editor_aliases.values().any(|b| b.slot == binding.slot);
            let slot_pane_alive = self
                .layouts
                .values()
                .any(|l| l.group.find_pane(binding.slot));
            if !aliased && !slot_pane_alive {
                self.release_editor_slot(binding.slot, cx);
            }
            return;
        }
        if !self.editors.contains_key(&pane) {
            return;
        }
        // The slot pane closed. If alias panes still show the document, keep
        // the view and drop only the slot pane's own group.
        if self.editor_aliases.values().any(|b| b.slot == pane) {
            if let Some(editor) = self.editors.get(&pane).cloned() {
                editor.update(cx, |e, cx| {
                    let first = e.first_group();
                    e.remove_group(first, cx);
                });
            }
            return;
        }
        self.release_editor_slot(pane, cx);
    }

    /// Final teardown of an editor document and everything bound to it.
    fn release_editor_slot(&mut self, slot: u64, cx: &mut Context<Self>) {
        if self.project_search_owner == Some(slot) {
            self.project_search_owner = None;
            self.project_search_task.take();
        }
        if let Some(editor) = self.editors.remove(&slot) {
            editor.update(cx, |editor, _cx| editor.prepare_for_close());
        }
        if let Some(re) = self.remote_edits.remove(&slot) {
            let temp = re.temp_path;
            let service = self.ssh_remote_file.clone();
            self.tokio.spawn(async move {
                let _ = service.cleanup_remote_edit_temp(temp).await;
            });
        }
    }

    /// An editor group took focus (a mirror pane was clicked): make its pane
    /// the focused pane of its tab.
    pub(crate) fn on_editor_group_focused(
        &mut self,
        slot: u64,
        group: labonair_editor::EditorGroupId,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.pane_of_editor_group(slot, group, cx) else {
            return;
        };
        if let Some(tab) = self.tab_of_pane(pane) {
            if let Some(layout) = self.layouts.get_mut(&tab) {
                layout.set_active(pane);
            }
        }
        cx.notify();
    }
}
