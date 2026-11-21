// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Status-bar item contracts.
//!
//! Each item chooses the left or right side and may describe how it hides
//! itself.
//!
//! # Extensibility
//!
//! The status bar renders **purely** from [`StatusItemRegistry`] — it is keyed
//! by an arbitrary `&'static str` id and an [`Arc`] constructor closure, and
//! has *no* dependency on the panel system. Application composition only
//! inserts owner-built contributions; any crate can build an entity that
//! implements [`StatusItem`] and call
//! [`StatusItemRegistry::register`] on the workspace's registry to add a new
//! status-bar widget. (The only panel coupling is the three `dock-buttons-*`
//! groups, which happen to iterate the panel registry.)
//!
//! A registered item is not limited to the shared "Move left / Move right /
//! Hide" right-click menu: it can contribute its own rows through
//! [`StatusItem::status_menu_entries`], which `StatusBar` merges into that same
//! single menu (so a widget with custom actions — e.g. the CWD breadcrumb's
//! "Copy path" — never opens a second, competing context menu).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AnyElement, AnyView, EntityId, Global, SharedString};
use labonair_gpui_ext::prelude::*;

/// Click handler for a [`StatusMenuEntry::Action`] — runs on the main thread
/// when the row is clicked; the status bar closes the menu afterwards.
pub type StatusMenuHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// A custom row a [`StatusItem`] contributes to its own status-bar right-click
/// menu, merged **above** the shared "Move left / Move right / Hide" block by
/// `StatusBar`.
///
/// Deliberately ui-kit-free so this trait can stay in this leaf crate; the
/// status bar maps each entry onto a `labonair_ui_kit::MenuItem`. Rebuilt every
/// time the menu opens, so `Rc` (main-thread only) is enough for the handler.
pub enum StatusMenuEntry {
    /// A clickable action row.
    Action {
        /// Element id, unique within the menu.
        id: SharedString,
        /// Row label.
        label: SharedString,
        /// Draw a leading check mark (radio / toggle state).
        checked: bool,
        /// Dim the row and ignore clicks.
        disabled: bool,
        /// Runs on click; the status bar closes the menu afterwards.
        on_click: StatusMenuHandler,
    },
    /// A horizontal divider.
    Separator,
    /// A non-interactive section heading.
    Label(SharedString),
}

impl StatusMenuEntry {
    /// A plain action row.
    pub fn action(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self::Action {
            id: id.into(),
            label: label.into(),
            checked: false,
            disabled: false,
            on_click: Rc::new(on_click),
        }
    }

    /// Set the leading check mark.
    pub fn checked(mut self, value: bool) -> Self {
        if let Self::Action { checked, .. } = &mut self {
            *checked = value;
        }
        self
    }

    /// Dim + disable the row.
    pub fn disabled(mut self, value: bool) -> Self {
        if let Self::Action { disabled, .. } = &mut self {
            *disabled = value;
        }
        self
    }
}

/// Which side of the status bar an item defaults to.
///
/// `statusBarItemPlacements` (`docs/architecture.md` §7) has
/// only left/right, so this enum has exactly two variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusSide {
    /// Left cluster — by default the panel toggles.
    Left,
    /// Right cluster — by default the info dropdowns.
    Right,
}

/// A user-chosen override of one item's side / visibility (T18-005),
/// persisted through `statusBarItemPlacements`. Absent from
/// [`StatusItemRegistry`]'s override table means "use the compiled-in
/// [`StatusItemRegistration::default_side`] and stay visible".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusPlacement {
    pub side: StatusSide,
    pub hidden: bool,
}

/// Describes how a status-bar item can be hidden by the user.
///
/// The owner supplies a callback against the running `App`; persistence stays
/// outside this contract.
#[derive(Clone)]
pub struct StatusItemHide {
    hide: Arc<dyn Fn(&mut App) + Send + Sync>,
}

impl StatusItemHide {
    /// Wrap a hide action.
    pub fn new(hide: impl Fn(&mut App) + Send + Sync + 'static) -> Self {
        Self {
            hide: Arc::new(hide),
        }
    }

    /// Run the hide action.
    pub fn apply(&self, cx: &mut App) {
        (self.hide)(cx);
    }
}

/// An item rendered into the status bar as a GPUI view.
///
/// Workspace state is observed directly. The `render_status` method avoids
/// colliding with [`Render::render`].
pub trait StatusItem: Render {
    /// Stable id, used as the key in the placement/hidden settings map.
    fn id(&self) -> &'static str;

    /// Side the item shows on until the user moves it.
    fn default_side(&self) -> StatusSide;

    /// Stable-sort key within a side (lower renders further from the centre).
    fn order(&self) -> i32 {
        0
    }

    /// Logical group within a side (T18-004): the status bar draws a divider
    /// between two consecutive items whose `group` differs, and none within a
    /// group. Groups are only ever compared for equality, so any stable
    /// numbering works; 0 (the default) puts every item that doesn't opt in
    /// into one undivided group.
    fn group(&self) -> u32 {
        0
    }

    /// Whether this item currently has nothing to show (e.g. no active
    /// update, no open transfers) and would otherwise render an empty slot.
    /// The status bar omits such items entirely — including from its
    /// group-divider calculation, so a transiently-empty item never leaves
    /// behind a divider that separates nothing. Default: never empty.
    fn is_empty(&self, cx: &App) -> bool {
        let _ = cx;
        false
    }

    /// Render the item's content for the status bar row.
    fn render_status(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement;

    /// Extra rows this item contributes to its status-bar right-click menu,
    /// rendered above the shared "Move left / Move right / Hide" block (see
    /// [`StatusMenuEntry`]). Called every time the menu opens. Default: none.
    fn status_menu_entries(&mut self, cx: &mut Context<Self>) -> Vec<StatusMenuEntry> {
        let _ = cx;
        Vec::new()
    }

    /// Whether the user is offered a "hide" affordance for this item. The
    /// persistence of that choice lands in T18-005; here it is only advisory.
    fn hideable(&self) -> bool {
        true
    }

    /// Notification hook for "the active workspace tab changed" — used by
    /// context-sensitive items (CWD breadcrumb, editor cursor). Optional; most
    /// items observe their own dependencies instead.
    fn on_active_tab_changed(&mut self, cx: &mut Context<Self>) {
        let _ = cx;
    }

    /// Notification that Workspace state used by this item may have changed.
    /// Active-editor status items use this to refresh after cursor/selection
    /// updates as well as tab changes.
    fn on_workspace_changed(&mut self, cx: &mut Context<Self>) {
        let _ = cx;
    }

    /// How this item hides itself, or `None` if it is inherently conditional on
    fn hide(&self, cx: &App) -> Option<StatusItemHide> {
        let _ = cx;
        None
    }
}

/// Object-safe view of a [`StatusItem`] entity.
///
/// The status bar stores `Arc<dyn StatusItemHandle>` and forwards through each item's
/// `Entity`.
pub trait StatusItemHandle: Send + Sync {
    /// Entity id of the underlying item view.
    fn item_id(&self) -> EntityId;
    /// See [`StatusItem::id`].
    fn id(&self, cx: &App) -> &'static str;
    /// See [`StatusItem::default_side`].
    fn default_side(&self, cx: &App) -> StatusSide;
    /// See [`StatusItem::order`].
    fn order(&self, cx: &App) -> i32;
    /// See [`StatusItem::group`].
    fn group(&self, cx: &App) -> u32;
    /// See [`StatusItem::hideable`].
    fn hideable(&self, cx: &App) -> bool;
    /// See [`StatusItem::is_empty`].
    fn is_empty(&self, cx: &App) -> bool;
    /// See [`StatusItem::status_menu_entries`].
    fn status_menu_entries(&self, cx: &mut App) -> Vec<StatusMenuEntry>;
    /// See [`StatusItem::on_active_tab_changed`].
    fn on_active_tab_changed(&self, cx: &mut App);
    /// See [`StatusItem::on_workspace_changed`].
    fn on_workspace_changed(&self, cx: &mut App);
    /// See [`StatusItem::hide`].
    fn hide(&self, cx: &App) -> Option<StatusItemHide>;
    /// Type-erased view, for rendering inside the status bar.
    fn to_any(&self) -> AnyView;
}

impl<T: StatusItem + 'static> StatusItemHandle for Entity<T> {
    fn item_id(&self) -> EntityId {
        self.entity_id()
    }

    fn id(&self, cx: &App) -> &'static str {
        self.read(cx).id()
    }

    fn default_side(&self, cx: &App) -> StatusSide {
        self.read(cx).default_side()
    }

    fn order(&self, cx: &App) -> i32 {
        self.read(cx).order()
    }

    fn group(&self, cx: &App) -> u32 {
        self.read(cx).group()
    }

    fn hideable(&self, cx: &App) -> bool {
        self.read(cx).hideable()
    }

    fn is_empty(&self, cx: &App) -> bool {
        self.read(cx).is_empty(cx)
    }

    fn status_menu_entries(&self, cx: &mut App) -> Vec<StatusMenuEntry> {
        self.update(cx, |item, cx| item.status_menu_entries(cx))
    }

    fn on_active_tab_changed(&self, cx: &mut App) {
        self.update(cx, |item, cx| item.on_active_tab_changed(cx));
    }

    fn on_workspace_changed(&self, cx: &mut App) {
        self.update(cx, |item, cx| item.on_workspace_changed(cx));
    }

    fn hide(&self, cx: &App) -> Option<StatusItemHide> {
        self.read(cx).hide(cx)
    }

    fn to_any(&self) -> AnyView {
        AnyView::from(self.clone())
    }
}

/// Type-erased, cheaply cloned handle to a registered status item.
pub type AnyStatusItemHandle = Arc<dyn StatusItemHandle>;

/// Builds a fresh status-item view. Stored in [`StatusItemRegistry`].
pub type StatusItemConstructor =
    Arc<dyn Fn(&mut Window, &mut App) -> AnyStatusItemHandle + Send + Sync>;

/// One status-item type's registration record.
pub struct StatusItemRegistration {
    /// [`StatusItem::id`] of the registered type.
    pub id: &'static str,
    /// Side the item shows on before the user moves it.
    pub default_side: StatusSide,
    /// Stable-sort key within a side (see [`StatusItem::order`]).
    pub order: i32,
    /// Logical group within a side (see [`StatusItem::group`]).
    pub group: u32,
    /// Constructor invoked lazily when the status bar is built.
    pub build: StatusItemConstructor,
}

/// The set of every status-item type known to the running app.
///
/// Same rationale as [`crate::PanelRegistry`]: capability modules build typed
/// contributions and application composition inserts them once; the
/// workspace's status bar reads them back.
#[derive(Default)]
pub struct StatusItemRegistry {
    items: Vec<StatusItemRegistration>,
    /// User overrides (T18-005), keyed by [`StatusItem::id`]. Loaded from the
    /// persisted `statusBarItemPlacements` blob by the workspace layer;
    /// merged over `default_side` by [`Self::resolve_side`] /
    /// [`Self::is_hidden`].
    overrides: HashMap<String, StatusPlacement>,
}

impl StatusItemRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a status-item type. A later registration with the same `id`
    /// replaces the earlier one.
    pub fn register(&mut self, registration: StatusItemRegistration) {
        match self.items.iter().position(|i| i.id == registration.id) {
            Some(index) => self.items[index] = registration,
            None => self.items.push(registration),
        }
    }

    /// All registrations, in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &StatusItemRegistration> {
        self.items.iter()
    }

    /// Registrations whose `default_side` equals `side`.
    pub fn for_side(&self, side: StatusSide) -> impl Iterator<Item = &StatusItemRegistration> {
        self.items.iter().filter(move |i| i.default_side == side)
    }

    /// Look up a registration by [`StatusItem::id`].
    pub fn get(&self, id: &str) -> Option<&StatusItemRegistration> {
        self.items.iter().find(|i| i.id == id)
    }

    /// The side an item renders on: the user's override if one is set
    /// (T18-005), else its registered
    /// [`default_side`](StatusItemRegistration::default_side).
    pub fn resolve_side(&self, id: &str) -> StatusSide {
        if let Some(placement) = self.overrides.get(id) {
            return placement.side;
        }
        self.get(id).map_or(StatusSide::Right, |r| r.default_side)
    }

    /// Whether the user has hidden this item (T18-005). Items with no
    /// override are visible.
    pub fn is_hidden(&self, id: &str) -> bool {
        self.overrides.get(id).is_some_and(|p| p.hidden)
    }

    /// The user's raw override for `id`, if any.
    pub fn override_for(&self, id: &str) -> Option<StatusPlacement> {
        self.overrides.get(id).copied()
    }

    /// Replace the whole override table (loaded from the persisted blob).
    pub fn set_overrides(&mut self, overrides: HashMap<String, StatusPlacement>) {
        self.overrides = overrides;
    }

    /// Merge a partial change (a right-click "move left/right" / "hide"
    /// action) into one item's override, defaulting the unset half to the
    /// item's current resolved placement, and return the result so the
    /// caller can persist it.
    pub fn set_override(
        &mut self,
        id: &str,
        side: Option<StatusSide>,
        hidden: Option<bool>,
    ) -> StatusPlacement {
        let current = self.override_for(id).unwrap_or_else(|| StatusPlacement {
            side: self.resolve_side(id),
            hidden: self.is_hidden(id),
        });
        let updated = StatusPlacement {
            side: side.unwrap_or(current.side),
            hidden: hidden.unwrap_or(current.hidden),
        };
        self.overrides.insert(id.to_string(), updated);
        updated
    }

    /// Number of registered status-item types.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether no status-item type is registered.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Global for StatusItemRegistry {}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_item(id: &'static str, side: StatusSide) -> StatusItemRegistration {
        StatusItemRegistration {
            id,
            default_side: side,
            order: 0,
            group: 0,
            // Never invoked by the registry's bookkeeping methods.
            build: Arc::new(|_, _| unreachable!("stub constructor")),
        }
    }

    #[test]
    fn register_and_lookup() {
        let mut reg = StatusItemRegistry::new();
        assert!(reg.is_empty());
        reg.register(stub_item("notifications", StatusSide::Right));
        reg.register(stub_item("cwd", StatusSide::Right));
        assert_eq!(reg.len(), 2);
        assert!(reg.get("cwd").is_some());
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn register_replaces_duplicate_id() {
        let mut reg = StatusItemRegistry::new();
        reg.register(stub_item("updater", StatusSide::Left));
        reg.register(stub_item("updater", StatusSide::Right));
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("updater").unwrap().default_side, StatusSide::Right);
    }

    #[test]
    fn for_side_filters() {
        let mut reg = StatusItemRegistry::new();
        reg.register(stub_item("toggles", StatusSide::Left));
        reg.register(stub_item("notifications", StatusSide::Right));
        reg.register(stub_item("cwd", StatusSide::Right));
        let right: Vec<_> = reg.for_side(StatusSide::Right).map(|i| i.id).collect();
        assert_eq!(right, ["notifications", "cwd"]);
        assert_eq!(reg.for_side(StatusSide::Left).count(), 1);
    }

    #[test]
    fn resolve_side_uses_default_side() {
        let mut reg = StatusItemRegistry::new();
        reg.register(stub_item("toggles", StatusSide::Left));
        reg.register(stub_item("cwd", StatusSide::Right));
        assert_eq!(reg.resolve_side("toggles"), StatusSide::Left);
        assert_eq!(reg.resolve_side("cwd"), StatusSide::Right);
        // Unknown id falls back to the right cluster.
        assert_eq!(reg.resolve_side("missing"), StatusSide::Right);
    }

    #[test]
    fn resolve_side_and_is_hidden_prefer_override() {
        let mut reg = StatusItemRegistry::new();
        reg.register(stub_item("cwd", StatusSide::Right));
        assert_eq!(reg.resolve_side("cwd"), StatusSide::Right);
        assert!(!reg.is_hidden("cwd"));

        reg.set_overrides(HashMap::from([(
            "cwd".to_string(),
            StatusPlacement {
                side: StatusSide::Left,
                hidden: true,
            },
        )]));
        assert_eq!(reg.resolve_side("cwd"), StatusSide::Left);
        assert!(reg.is_hidden("cwd"));
        // An item with no override still falls back to its default.
        reg.register(stub_item("notifications", StatusSide::Right));
        assert_eq!(reg.resolve_side("notifications"), StatusSide::Right);
        assert!(!reg.is_hidden("notifications"));
    }

    #[test]
    fn set_override_merges_partial_changes() {
        let mut reg = StatusItemRegistry::new();
        reg.register(stub_item("cwd", StatusSide::Right));

        // Only moving the side leaves `hidden` at its current (false) value.
        let p = reg.set_override("cwd", Some(StatusSide::Left), None);
        assert_eq!(p.side, StatusSide::Left);
        assert!(!p.hidden);
        assert_eq!(reg.resolve_side("cwd"), StatusSide::Left);

        // Only hiding leaves the side untouched.
        let p = reg.set_override("cwd", None, Some(true));
        assert_eq!(p.side, StatusSide::Left);
        assert!(p.hidden);
        assert!(reg.is_hidden("cwd"));

        // Unhiding again keeps the moved side.
        let p = reg.set_override("cwd", None, Some(false));
        assert_eq!(p.side, StatusSide::Left);
        assert!(!p.hidden);
    }
}
