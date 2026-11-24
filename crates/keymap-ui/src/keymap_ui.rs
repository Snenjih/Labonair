// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Native keymap management surface.
//!
//! This crate owns presentation only. The keymap domain supplies the
//! lossless document, diagnostics, effective command rows, override state, and
//! conflict data; the tab owner (`labonair-workspace`) constructs and hosts
//! [`KeymapManagementView`] as a workspace tab, supplies the composition
//! callback that opens the raw JSONC document, and drives [`reload`] when the
//! file changes on disk. No command execution, file parsing, tab lifecycle, or
//! feature state belongs here.

mod keystroke;

use std::time::{Duration, Instant};

use gpui::{
    div, prelude::FluentBuilder, px, AnyElement, App, AppContext, ClickEvent, Context, Entity,
    FocusHandle, Focusable, InteractiveElement, IntoElement, Keystroke, MouseButton, ParentElement,
    Pixels, Point, Render, SharedString, StatefulInteractiveElement, Styled, Subscription,
    WeakEntity, Window,
};

use labonair_command_palette_core::{CommandContext, CommandDescriptor, CommandId};
use labonair_keymap::{
    adapter, file, management,
    management::{KeymapCommandRow, KeymapManagementSnapshot, OverrideState, RowFilter},
    runtime,
};
use labonair_notifications::{notification_center, Notification};
use labonair_settings::{
    SettingsSurface, SettingsSurfaceContribution, SettingsSurfaceId, SettingsSurfacePage,
    SettingsSurfaceRegistry,
};
use labonair_theme::ThemeStore;
use labonair_ui_kit::{
    banner, button, context_menu, segmented_control, text_field_surface_sized, text_input,
    ButtonSize, ButtonVariant, InputEvent, InputState, MenuItem, Palette, SegmentVariant, Severity,
    TextFieldSize, TextFieldState,
};

/// Composition callback that opens the raw user `keymap.json` document. Its
/// tab lifecycle belongs to the workspace, so the owner injects it.
pub type OpenRawCallback = Box<dyn FnMut(&mut Window, &mut App) + 'static>;

/// Contribute the Keymap-owned editor to the pinned Settings navigation.
pub fn register_settings_surfaces(
    registry: &mut SettingsSurfaceRegistry,
    open_keymap: impl Fn(&mut App) + 'static,
) -> Result<(), String> {
    registry.register(SettingsSurfaceContribution::new(
        SettingsSurface::new(
            SettingsSurfaceId::new("keymap"),
            "Keyboard shortcuts",
            "Browse and edit the shortcuts provided by Labonair.",
            "Open keymap editor…",
            SettingsSurfacePage::new_page(
                "keymap",
                "Keymap",
                "Keyboard shortcuts",
                labonair_settings::FIRST_OWNER_PAGE_INDEX,
            ),
        ),
        open_keymap,
    ))
}

/// The keymap tab's view state.
pub struct KeymapManagementView {
    theme: Entity<ThemeStore>,
    descriptors: Vec<CommandDescriptor>,
    snapshot: Option<KeymapManagementSnapshot>,
    load_error: Option<String>,
    open_raw: Option<OpenRawCallback>,

    // Browsing / filtering.
    filter: String,
    row_filter: RowFilter,
    search: Entity<InputState>,
    _search_subscription: Subscription,

    // Inline recording state for one pill, and the right-click menu.
    editor: Option<BindingEditor>,
    capture_subscription: Option<Subscription>,
    menu: Option<RowMenu>,

    // Live external-change handling.
    external_change_pending: bool,
    last_self_write: Option<Instant>,

    focus: FocusHandle,
}

/// Right-click menu target: a command and, when a pill was hit, its index.
struct RowMenu {
    command: CommandId,
    binding: Option<usize>,
    anchor: Point<Pixels>,
}

/// The in-flight edit of one binding (replace, add, or remove).
struct BindingEditor {
    command: CommandId,
    action: String,
    /// Portable context this binding is scoped to (`None` = global).
    context: Option<CommandContext>,
    /// Chords unbound when the edit is written: the replaced/removed one, or
    /// nothing when adding an additional binding.
    original_bindings: Vec<String>,
    /// Index of the pill being replaced; `None` when adding a new binding.
    binding: Option<usize>,
    recording: bool,
    captured: Vec<Keystroke>,
    error: Option<String>,
    saving: bool,
}

/// Chords an edit unbinds: only the targeted one when replacing, none when
/// adding an additional binding.
fn replaced_chords(existing: &[String], index: Option<usize>) -> Vec<String> {
    index
        .and_then(|index| existing.get(index))
        .cloned()
        .into_iter()
        .collect()
}

/// Group row sections in first-seen order, keeping row indices.
fn group_sections(sections: &[String]) -> Vec<(String, Vec<usize>)> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (index, section) in sections.iter().enumerate() {
        match groups.iter_mut().find(|(name, _)| name == section) {
            Some((_, indices)) => indices.push(index),
            None => groups.push((section.clone(), vec![index])),
        }
    }
    groups
}

impl Focusable for KeymapManagementView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl KeymapManagementView {
    pub fn new(
        theme: Entity<ThemeStore>,
        descriptors: Vec<CommandDescriptor>,
        open_raw: OpenRawCallback,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search shortcuts…"));
        let subscription = cx.subscribe(&search, |view, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                view.filter = input.read(cx).value().to_string();
                cx.notify();
            }
        });
        let mut view = Self {
            theme,
            descriptors,
            snapshot: None,
            load_error: None,
            open_raw: Some(open_raw),
            filter: String::new(),
            row_filter: RowFilter::All,
            search,
            _search_subscription: subscription,
            editor: None,
            capture_subscription: None,
            menu: None,
            external_change_pending: false,
            last_self_write: None,
            focus: cx.focus_handle(),
        };
        view.load_snapshot(cx);
        view
    }

    // ── loading ───────────────────────────────────────────────────────────

    fn load_snapshot(&mut self, cx: &mut Context<Self>) {
        let descriptors = self.descriptors.clone();
        let load = cx.background_executor().spawn(async move {
            let known_actions = management::known_actions(descriptors.iter());
            let document = file::read_user_keymap_document(&known_actions)?;
            let runtime = adapter::load_descriptors(descriptors.iter());
            Ok::<_, String>(management::snapshot(descriptors.iter(), &runtime, document))
        });
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_, cx| match load.await {
            Ok(snapshot) => {
                let _ = entity.update(cx, |view, cx| {
                    view.publish_diagnostics(&snapshot, cx);
                    view.snapshot = Some(snapshot);
                    view.load_error = None;
                    view.external_change_pending = false;
                    cx.notify();
                });
            }
            Err(error) => {
                let _ = entity.update(cx, |view, cx| {
                    view.publish_error(
                        "Keymap could not be loaded",
                        "The active keymap was not changed.",
                        error.clone(),
                        cx,
                    );
                    view.load_error = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Re-read `keymap.json` after an external change (raw editor, another
    /// editor, a sync). Deferred while a rebind is in progress so an in-flight
    /// edit is never discarded; a reload that immediately follows this view's
    /// own write is a no-op (the write path already refreshed the snapshot).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self
            .last_self_write
            .is_some_and(|at| at.elapsed() < Duration::from_millis(1500))
        {
            return;
        }
        if self.editor.is_some() || self.capture_subscription.is_some() {
            self.external_change_pending = true;
            cx.notify();
            return;
        }
        self.load_snapshot(cx);
    }

    fn publish_error(
        &self,
        title: &'static str,
        summary: &'static str,
        details: String,
        cx: &mut Context<Self>,
    ) {
        if !cx.has_global::<labonair_notifications::GlobalNotificationCenter>() {
            return;
        }
        notification_center(cx).update(cx, |center, cx| {
            center.push(
                Notification::error(title, summary)
                    .details(details.clone())
                    .dedupe_key(format!("keymap:{title}:{details}")),
                cx,
            );
        });
    }

    fn publish_diagnostics(&mut self, snapshot: &KeymapManagementSnapshot, cx: &mut Context<Self>) {
        if snapshot.issues.is_empty() {
            return;
        }
        let details = snapshot
            .issues
            .iter()
            .map(|issue| format!("line {}: {}", issue.line, issue.message))
            .collect::<Vec<_>>()
            .join("\n");
        let error_count = snapshot
            .issues
            .iter()
            .filter(|issue| matches!(issue.severity, file::Severity::Error))
            .count();
        let summary = format!(
            "{} error(s), {} warning(s) found in keymap.json.",
            error_count,
            snapshot.issues.len() - error_count
        );
        if !cx.has_global::<labonair_notifications::GlobalNotificationCenter>() {
            return;
        }
        notification_center(cx).update(cx, |center, cx| {
            let notification = if error_count > 0 {
                Notification::error("Keymap diagnostics", summary)
            } else {
                Notification::warning("Keymap diagnostics", summary)
            };
            center.push(
                notification
                    .details(details.clone())
                    .dedupe_key(format!("keymap-diagnostics:{details}")),
                cx,
            );
        });
    }

    // ── editing ───────────────────────────────────────────────────────────

    fn on_key(&mut self, event: &gpui::KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && (self.editor.is_some() || self.menu.is_some()) {
            self.menu = None;
            self.close_editor(cx);
            cx.stop_propagation();
        }
    }

    /// Start editing `command`: replace the pill at `binding_index`, or add an
    /// additional binding when `None`.
    fn begin_edit(
        &mut self,
        command: CommandId,
        binding_index: Option<usize>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.stop_recording();
        self.menu = None;
        let Some(row) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.rows.iter().find(|row| row.command == command))
        else {
            return false;
        };
        let binding_index = binding_index.filter(|index| *index < row.effective_bindings.len());
        let context = match binding_index {
            Some(index) => row.effective_bindings[index]
                .context
                .as_deref()
                .and_then(context_from_label),
            None => row.contexts.first().copied(),
        };
        let chords: Vec<String> = row
            .effective_bindings
            .iter()
            .map(|binding| binding.keystrokes.clone())
            .collect();
        self.editor = Some(BindingEditor {
            command,
            action: command.action_name().to_string(),
            context,
            original_bindings: replaced_chords(&chords, binding_index),
            binding: binding_index,
            recording: false,
            captured: Vec::new(),
            error: None,
            saving: false,
        });
        cx.notify();
        true
    }

    /// Start recording a replacement (`Some`) or an additional binding (`None`).
    fn record_binding(
        &mut self,
        command: CommandId,
        binding_index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if self.begin_edit(command, binding_index, cx) {
            self.start_recording(cx);
        }
    }

    fn close_editor(&mut self, cx: &mut Context<Self>) {
        self.stop_recording();
        self.editor = None;
        if self.external_change_pending {
            self.load_snapshot(cx);
        }
        cx.notify();
    }

    fn start_recording(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        editor.recording = true;
        editor.captured.clear();
        editor.error = None;
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        self.capture_subscription = Some(cx.intercept_keystrokes(move |event, _window, cx| {
            let keystroke = event.keystroke.clone();
            let _ = weak.update(cx, |view, cx| view.on_captured(keystroke, cx));
            cx.stop_propagation();
        }));
        cx.notify();
    }

    fn stop_recording(&mut self) {
        self.capture_subscription = None;
        if let Some(editor) = self.editor.as_mut() {
            editor.recording = false;
        }
    }

    /// The first non-modifier key press is the binding; Esc cancels.
    fn on_captured(&mut self, keystroke: Keystroke, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        if !editor.recording {
            return;
        }
        if keystroke.key == "escape" {
            self.close_editor(cx);
            return;
        }
        if keystroke::is_modifier_only(&keystroke) {
            return;
        }
        editor.captured = vec![keystroke];
        self.write_edit(false, cx);
    }

    fn unbind_edit(&mut self, cx: &mut Context<Self>) {
        self.write_edit(true, cx);
    }

    fn write_edit(&mut self, unbind: bool, cx: &mut Context<Self>) {
        self.stop_recording();
        let Some(snapshot) = self.snapshot.clone() else {
            return;
        };
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        if editor.saving {
            return;
        }
        let candidate = if unbind {
            String::new()
        } else {
            keystroke::to_binding_string(&editor.captured)
        };

        if !unbind {
            if candidate.is_empty() {
                editor.error = Some("Press a key combination.".to_string());
                cx.notify();
                return;
            }
            let context_name = editor.context.map(runtime::context_name);
            if let Some(conflict) = snapshot.conflict(editor.command, context_name, &candidate) {
                editor.error = Some(format!(
                    "Already bound to “{}” in {}.",
                    conflict.title,
                    conflict
                        .context
                        .as_deref()
                        .unwrap_or(runtime::GLOBAL_CONTEXT_LABEL)
                ));
                cx.notify();
                return;
            }
        }

        let context_name = editor
            .context
            .map(runtime::context_name)
            .map(str::to_string);
        let replacement = (!candidate.is_empty()).then_some(candidate);
        match file::append_user_binding_override(
            &snapshot.document.source,
            context_name.as_deref(),
            &editor.original_bindings,
            &editor.action,
            replacement.as_deref(),
        ) {
            Ok(updated) => {
                editor.saving = true;
                editor.error = None;
                self.persist(updated, cx);
            }
            Err(error) => {
                editor.error = Some(error);
                cx.notify();
            }
        }
    }

    fn reset_to_default(&mut self, cx: &mut Context<Self>) {
        self.stop_recording();
        let Some(snapshot) = self.snapshot.clone() else {
            return;
        };
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        if editor.saving {
            return;
        }
        let context_name = editor
            .context
            .map(runtime::context_name)
            .map(str::to_string);
        let action = editor.action.clone();
        let defaults: Vec<String> = self
            .descriptors
            .iter()
            .find(|descriptor| descriptor.id == editor.command)
            .map(|descriptor| {
                descriptor
                    .default_bindings
                    .iter()
                    .filter(|default| {
                        default
                            .context
                            .map(runtime::context_name)
                            .map(str::to_string)
                            == context_name
                    })
                    .map(|default| default.keystrokes.clone())
                    .collect()
            })
            .unwrap_or_default();

        match file::remove_user_binding_override(
            &snapshot.document.source,
            context_name.as_deref(),
            &action,
            &defaults,
        ) {
            Ok(updated) => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.saving = true;
                    editor.error = None;
                }
                self.persist(updated, cx);
            }
            Err(error) => {
                if let Some(editor) = self.editor.as_mut() {
                    editor.error = Some(error.clone());
                }
                self.publish_error(
                    "Could not reset the binding",
                    "Edit keymap.json directly to change it.",
                    error,
                    cx,
                );
                cx.notify();
            }
        }
    }
    fn persist(&mut self, updated: String, cx: &mut Context<Self>) {
        let save = cx
            .background_executor()
            .spawn(async move { file::save_user_keymap_document(&updated) });
        let entity = cx.entity().downgrade();
        cx.spawn(async move |_, cx| match save.await {
            Ok(_) => {
                let _ = entity.update(cx, |view, cx| {
                    view.last_self_write = Some(Instant::now());
                    view.editor = None;
                    view.capture_subscription = None;
                    view.load_snapshot(cx);
                    cx.notify();
                });
            }
            Err(error) => {
                let _ = entity.update(cx, |view, cx| {
                    if let Some(editor) = view.editor.as_mut() {
                        editor.saving = false;
                        editor.error = Some(format!("Could not save keymap.json: {error}"));
                    }
                    view.publish_error(
                        "Could not save keymap.json",
                        "The keyboard binding was not saved.",
                        error,
                        cx,
                    );
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn open_raw_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(callback) = self.open_raw.as_mut() {
            callback(window, cx);
        }
    }
}
fn chord_chips(chord: &str) -> Vec<String> {
    chord
        .split_whitespace()
        .flat_map(labonair_keymap::keystroke_tokens)
        .collect()
}

fn context_from_label(label: &str) -> Option<CommandContext> {
    match label {
        "Editor" => Some(CommandContext::Editor),
        "EditorVim" | "Vim Editor" => Some(CommandContext::EditorVim),
        "EditorHelix" | "Helix Editor" => Some(CommandContext::EditorHelix),
        "EditorSnippetNext" | "Snippet Next Tabstop" => Some(CommandContext::EditorSnippetNext),
        "EditorSnippetPrevious" | "Snippet Previous Tabstop" => {
            Some(CommandContext::EditorSnippetPrevious)
        }
        "Preview" => Some(CommandContext::Preview),
        "Diff" => Some(CommandContext::Diff),
        "Terminal" => Some(CommandContext::Terminal),
        "Sftp" | "SFTP" => Some(CommandContext::Sftp),
        "Home" => Some(CommandContext::Home),
        "SshTerminal" | "SSH Terminal" => Some(CommandContext::SshTerminal),
        _ => None,
    }
}

// ── rendering ──────────────────────────────────────────────────────────────

fn chord_label(chord: &str) -> String {
    chord_chips(chord).concat()
}

impl KeymapManagementView {
    fn render_pill(
        &self,
        row: &KeymapCommandRow,
        index: Option<usize>,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let command = row.command;
        let recording = self.editor.as_ref().is_some_and(|editor| {
            editor.recording && editor.command == command && editor.binding == index
        });
        let (label, variant) = match (recording, index) {
            (true, _) => ("Press keys…".to_string(), ButtonVariant::Filled),
            (false, Some(i)) => (
                chord_label(&row.effective_bindings[i].keystrokes),
                ButtonVariant::Outlined,
            ),
            (false, None) => ("None".to_string(), ButtonVariant::Subtle),
        };
        let slot = index.unwrap_or(usize::MAX);
        button(
            SharedString::from(format!(
                "keymap-pill-{}-{}",
                row.command.action_name(),
                slot
            )),
            palette,
            variant,
            ButtonSize::Sm,
        )
        .min_w(px(72.0))
        .child(
            div()
                .text_color(if index.is_none() && !recording {
                    palette.muted
                } else {
                    palette.fg
                })
                .child(SharedString::from(label)),
        )
        .on_click(cx.listener(move |this, event: &ClickEvent, _window, cx| {
            let modifiers = event.modifiers();
            // Shift/Cmd-click adds another binding instead of replacing.
            let additive = modifiers.shift || modifiers.platform;
            this.record_binding(command, if additive { None } else { index }, cx);
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &gpui::MouseDownEvent, _window, cx| {
                this.menu = Some(RowMenu {
                    command,
                    binding: index,
                    anchor: event.position,
                });
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .into_any_element()
    }

    fn render_row(
        &self,
        row: &KeymapCommandRow,
        first: bool,
        conflicted: bool,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let command = row.command;
        let pills: Vec<AnyElement> = if row.effective_bindings.is_empty() {
            vec![self.render_pill(row, None, palette, cx)]
        } else {
            (0..row.effective_bindings.len())
                .map(|index| self.render_pill(row, Some(index), palette, cx))
                .collect()
        };
        let error = self
            .editor
            .as_ref()
            .filter(|editor| editor.command == command)
            .and_then(|editor| editor.error.clone());
        let note = if conflicted {
            Some(("Overridden by another command", palette.warning))
        } else if management::row_override_state(row) == OverrideState::Customized {
            Some(("Modified", palette.info))
        } else {
            None
        };

        div()
            .id(SharedString::from(format!(
                "keymap-row-{}",
                command.action_name()
            )))
            .w_full()
            .px(px(14.0))
            .py(px(8.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .when(!first, |row| row.border_t_1().border_color(palette.border))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _window, cx| {
                    this.menu = Some(RowMenu {
                        command,
                        binding: None,
                        anchor: event.position,
                    });
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .child(SharedString::from(row.title.clone())),
                            )
                            .children(note.map(|(text, color)| {
                                div().text_size(px(10.0)).text_color(color).child(text)
                            })),
                    )
                    .child(div().flex().items_center().gap(px(6.0)).children(pills)),
            )
            .children(error.map(|error| {
                div()
                    .text_size(px(11.0))
                    .text_color(palette.error)
                    .child(SharedString::from(error))
            }))
            .into_any_element()
    }

    fn render_menu(&self, palette: Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let row = self
            .snapshot
            .as_ref()?
            .rows
            .iter()
            .find(|row| row.command == menu.command)?;
        let (command, binding) = (menu.command, menu.binding);
        let view = cx.entity();
        let dismiss = {
            let view = view.clone();
            move |_: &mut Window, cx: &mut App| {
                view.update(cx, |this, cx| {
                    this.menu = None;
                    cx.notify();
                })
            }
        };
        let resettable = management::row_override_state(row) != OverrideState::Default;
        let mut items = vec![
            MenuItem::new("reset", "Reset to default")
                .disabled(!resettable)
                .on_click({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            if this.begin_edit(command, binding, cx) {
                                this.reset_to_default(cx);
                            }
                        })
                    }
                }),
            MenuItem::new("add", "Add another binding").on_click({
                let view = view.clone();
                move |_, _, cx| view.update(cx, |this, cx| this.record_binding(command, None, cx))
            }),
        ];
        if binding.is_some() {
            items.push(MenuItem::separator());
            items.push(
                MenuItem::new("remove", "Remove binding")
                    .destructive()
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                if this.begin_edit(command, binding, cx) {
                                    this.unbind_edit(cx);
                                }
                            })
                        }
                    }),
            );
        }
        Some(context_menu(menu.anchor, palette, dismiss, items))
    }
}

impl Render for KeymapManagementView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::from_theme(self.theme.read(cx));

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(24.0))
            .pt(px(18.0))
            .pb(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(16.0)).child("Keymap"))
                    .child(
                        div()
                            .mt(px(3.0))
                            .text_size(px(11.0))
                            .text_color(palette.muted)
                            .child("Click a shortcut to rebind it. Shift-click adds another."),
                    ),
            )
            .child(
                button(
                    "keymap-edit-json",
                    palette,
                    ButtonVariant::Outlined,
                    ButtonSize::Sm,
                )
                .child("Edit keymap.json")
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.open_raw_document(window, cx);
                })),
            );

        // Search (left, fills the row) and mode filter (right) share one row.
        let search_state = if self.search.read(cx).focus_handle(cx).is_focused(window) {
            TextFieldState::Focused
        } else {
            TextFieldState::Normal
        };
        let search = text_field_surface_sized(
            "keymap-search",
            palette,
            search_state,
            TextFieldSize::Compact,
            text_input(&self.search, palette),
        )
        .flex_1()
        .min_w_0();
        let filters = segmented_control(
            "keymap-filter",
            palette,
            match self.row_filter {
                RowFilter::All => "all",
                RowFilter::Customized => "customized",
                RowFilter::Conflicts => "conflicts",
                RowFilter::Unbound => "unbound",
            },
        )
        .variant(SegmentVariant::Solid)
        .segments([
            ("all", "All"),
            ("customized", "Modified"),
            ("conflicts", "Conflicts"),
            ("unbound", "Unbound"),
        ])
        .on_select(cx.listener(|this, key: &SharedString, _window, cx| {
            this.row_filter = match key.as_ref() {
                "customized" => RowFilter::Customized,
                "conflicts" => RowFilter::Conflicts,
                "unbound" => RowFilter::Unbound,
                _ => RowFilter::All,
            };
            cx.notify();
        }));
        let toolbar = div()
            .px(px(24.0))
            .pb(px(12.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(search)
            .child(div().flex_shrink_0().child(filters));

        let mut root = div()
            .track_focus(&self.focus)
            .key_context("Keymap")
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.bg)
            .text_color(palette.fg)
            .child(header);

        if let Some(snapshot) = &self.snapshot {
            if !snapshot.issues.is_empty() {
                let has_error = snapshot
                    .issues
                    .iter()
                    .any(|issue| matches!(issue.severity, file::Severity::Error));
                let lines = snapshot
                    .issues
                    .iter()
                    .take(4)
                    .map(|issue| format!("line {}: {}", issue.line, issue.message))
                    .collect::<Vec<_>>()
                    .join("  ·  ");
                let more = snapshot.issues.len().saturating_sub(4);
                let text = if more > 0 {
                    format!("{lines}  · +{more} more")
                } else {
                    lines
                };
                root = root.child(
                    banner(
                        if has_error {
                            Severity::Error
                        } else {
                            Severity::Warning
                        },
                        palette,
                    )
                    .child(SharedString::from(text))
                    .child(
                        button(
                            "keymap-open-diagnostics",
                            palette,
                            ButtonVariant::Subtle,
                            ButtonSize::Xs,
                        )
                        .child("Open keymap.json")
                        .on_click(cx.listener(
                            |this, _: &ClickEvent, window, cx| {
                                this.open_raw_document(window, cx);
                            },
                        )),
                    ),
                );
            }
        }

        if self.external_change_pending {
            root = root.child(banner(Severity::Info, palette).child(SharedString::from(
                "keymap.json changed on disk — finish this edit to reload.",
            )));
        }

        root = root.child(toolbar);

        let body = if let Some(snapshot) = self.snapshot.clone() {
            let conflicts = snapshot.conflict_commands();
            let matched: Vec<KeymapCommandRow> = snapshot
                .search_filtered(&self.filter, self.row_filter)
                .into_iter()
                .cloned()
                .collect();
            let sections: Vec<String> = matched.iter().map(|row| row.section.clone()).collect();
            if matched.is_empty() {
                div()
                    .flex()
                    .flex_1()
                    .justify_center()
                    .py(px(42.0))
                    .text_size(px(12.0))
                    .text_color(palette.muted)
                    .child(SharedString::from(empty_message(self.row_filter)))
                    .into_any_element()
            } else {
                let mut column = div().flex().flex_col().gap(px(20.0)).max_w(px(760.0));
                for (name, indices) in group_sections(&sections) {
                    let rows: Vec<AnyElement> = indices
                        .iter()
                        .enumerate()
                        .map(|(position, index)| {
                            let row = &matched[*index];
                            self.render_row(
                                row,
                                position == 0,
                                conflicts.contains(&row.command),
                                palette,
                                cx,
                            )
                        })
                        .collect();
                    column = column.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .px(px(4.0))
                                    .text_size(px(11.0))
                                    .text_color(palette.muted)
                                    .child(SharedString::from(name)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .rounded(px(palette.radius.lg))
                                    .border_1()
                                    .border_color(palette.border)
                                    .overflow_hidden()
                                    .children(rows),
                            ),
                    );
                }
                div()
                    .id("keymap-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(24.0))
                    .pb(px(24.0))
                    .child(column)
                    .into_any_element()
            }
        } else if self.load_error.is_none() {
            div()
                .flex()
                .flex_1()
                .justify_center()
                .py(px(42.0))
                .text_size(px(12.0))
                .text_color(palette.muted)
                .child("Loading keymap…")
                .into_any_element()
        } else {
            div().flex().flex_1().into_any_element()
        };

        root.child(body).children(self.render_menu(palette, cx))
    }
}

fn empty_message(filter: RowFilter) -> &'static str {
    match filter {
        RowFilter::All => "No commands match the filter.",
        RowFilter::Customized => "You haven't changed any keybindings yet.",
        RowFilter::Conflicts => "No binding conflicts.",
        RowFilter::Unbound => "Every command with a default is bound.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keymap_settings_surface_is_registered_by_its_owner() {
        let mut registry = SettingsSurfaceRegistry::default();
        register_settings_surfaces(&mut registry, |_| {}).unwrap();
        let surface = registry.surface(SettingsSurfaceId::new("keymap")).unwrap();
        assert_eq!(surface.page.slug, "keymap");
        assert_eq!(
            surface.page.insert_at,
            Some(labonair_settings::FIRST_OWNER_PAGE_INDEX)
        );
    }

    #[test]
    fn keymap_view_type_is_ui_owned() {
        fn assert_focusable<T: Focusable>() {}
        assert_focusable::<KeymapManagementView>();
    }

    #[test]
    fn replace_unbinds_only_the_target_chord_and_add_unbinds_none() {
        let existing = vec!["cmd-a".to_string(), "cmd-b".to_string()];
        assert_eq!(replaced_chords(&existing, Some(1)), vec!["cmd-b"]);
        assert!(replaced_chords(&existing, None).is_empty());
        assert!(replaced_chords(&existing, Some(5)).is_empty());
    }

    #[test]
    fn sections_group_in_first_seen_order() {
        let sections: Vec<String> = ["File", "Edit", "File"].map(String::from).to_vec();
        assert_eq!(
            group_sections(&sections),
            vec![
                ("File".to_string(), vec![0, 2]),
                ("Edit".to_string(), vec![1])
            ]
        );
    }

    #[test]
    fn context_labels_round_trip() {
        for context in [
            CommandContext::Editor,
            CommandContext::EditorVim,
            CommandContext::EditorHelix,
            CommandContext::EditorSnippetNext,
            CommandContext::EditorSnippetPrevious,
            CommandContext::Preview,
            CommandContext::Diff,
            CommandContext::Terminal,
            CommandContext::Sftp,
            CommandContext::Home,
            CommandContext::SshTerminal,
        ] {
            assert_eq!(
                context_from_label(runtime::context_name(context)),
                Some(context)
            );
            assert_eq!(
                context_from_label(runtime::context_label(context)),
                Some(context)
            );
        }
    }
}
