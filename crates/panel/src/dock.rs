// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Panel + dock contracts.
//!
//! The fixed Labonair layout needs only panel position, sizing, lifecycle, and
//! status-bar toggle contracts. Flexible sizing, remote identifiers, and
//! activation priority are not part of this contract.

use std::sync::Arc;

use gpui::{AnyView, EntityId, Global};
use labonair_gpui_ext::prelude::*;

/// Position of a panel's dock in the workspace layout.
///
/// Conversions to settings-specific position types belong to the persistence
/// layer in `labonair-workspace` and `labonair-settings`, not this contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockPosition {
    /// Left edge dock.
    Left,
    /// Right edge dock.
    Right,
    /// Bottom edge dock.
    Bottom,
}

impl DockPosition {
    /// All positions in a stable order, used by "move panel to next dock".
    pub const ALL: [DockPosition; 3] = [
        DockPosition::Left,
        DockPosition::Right,
        DockPosition::Bottom,
    ];

    /// The next position in [`DockPosition::ALL`] order, wrapping around.
    pub fn next(self) -> DockPosition {
        match self {
            DockPosition::Left => DockPosition::Right,
            DockPosition::Right => DockPosition::Bottom,
            DockPosition::Bottom => DockPosition::Left,
        }
    }
}

/// The glyph shown on a panel's status-bar toggle.
///
/// This small closed enum avoids pulling `labonair-ui-kit` into the contracts
/// crate. It defines one variant per Labonair panel; `labonair-shell` maps each
/// variant to a concrete `IconName` when it renders the toggle
/// (`docs/architecture.md` §3, §5). If a
/// future panel needs a new icon, add a variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelIcon {
    /// File explorer.
    Explorer,
    /// Source-control (status / staging) panel.
    SourceControl,
    /// Command-snippets panel.
    Snippets,
    /// AI-chat panel.
    Ai,
    /// Workspace tab list (shown when `tabsLocation == "sidebar"`).
    Tabs,
}

/// Lifecycle and zoom events a panel emits to its host dock.
///
/// Defined here so the dock and panel crates agree on the variant set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelEvent {
    /// The panel became the active one in its dock.
    Activate,
    /// The panel asked to be closed / hidden.
    Close,
    /// The panel asked to take over the whole workspace area.
    ZoomIn,
    /// The panel asked to return to its normal dock size.
    ZoomOut,
}

/// A dockable side panel in the fixed Labonair layout.
///
/// The contract exposes the stable name, position, sizing, icon, and title
/// needed by the workspace. Type erasure for storage goes through
/// [`PanelHandle`] and [`AnyPanelHandle`].
pub trait Panel: Focusable + Render + Sized {
    /// Stable key used to persist this panel's dock/size across restarts.
    /// Must never change once shipped.
    fn persistent_name() -> &'static str;

    /// Human-readable title for the dock header and the toggle tooltip.
    fn title(&self, cx: &App) -> SharedString;

    /// Glyph for the status-bar toggle.
    fn icon(&self) -> PanelIcon;

    /// The dock this panel is currently shown in.
    fn position(&self, cx: &App) -> DockPosition;

    /// Whether this panel is allowed to move to `position`
    /// (e.g. a wide panel may forbid the bottom dock).
    fn position_is_valid(&self, position: DockPosition) -> bool;

    /// Move the panel to `position`. The host dock calls this after validating
    /// with [`Panel::position_is_valid`].
    fn set_position(&mut self, position: DockPosition, window: &mut Window, cx: &mut Context<Self>);

    /// Preferred size (width for left/right, height for bottom) before the user
    /// resizes the dock.
    fn default_size(&self, cx: &App) -> Pixels;

    /// Lower bound the dock resize handle must respect. `None` = dock default.
    fn min_size(&self) -> Option<Pixels> {
        None
    }
}

/// Object-safe view of a [`Panel`] entity.
///
/// The workspace stores `Arc<dyn PanelHandle>` rather than a concrete panel type, and every method
/// forwards through the panel's `Entity`. Only the subset matching the reduced
/// [`Panel`] trait is exposed.
pub trait PanelHandle: Send + Sync {
    /// Entity id of the underlying panel view.
    fn panel_id(&self) -> EntityId;
    /// See [`Panel::persistent_name`].
    fn persistent_name(&self) -> &'static str;
    /// See [`Panel::title`].
    fn title(&self, cx: &App) -> SharedString;
    /// See [`Panel::icon`].
    fn icon(&self, cx: &App) -> PanelIcon;
    /// See [`Panel::position`].
    fn position(&self, cx: &App) -> DockPosition;
    /// See [`Panel::position_is_valid`].
    fn position_is_valid(&self, position: DockPosition, cx: &App) -> bool;
    /// See [`Panel::set_position`].
    fn set_position(&self, position: DockPosition, window: &mut Window, cx: &mut App);
    /// See [`Panel::default_size`].
    fn default_size(&self, cx: &App) -> Pixels;
    /// See [`Panel::min_size`].
    fn min_size(&self, cx: &App) -> Option<Pixels>;
    /// Focus handle of the panel's root, for containment checks.
    fn focus_handle(&self, cx: &App) -> FocusHandle;
    /// Type-erased view, for rendering inside the dock.
    fn to_any(&self) -> AnyView;
}

impl<T: Panel + 'static> PanelHandle for Entity<T> {
    fn panel_id(&self) -> EntityId {
        self.entity_id()
    }

    fn persistent_name(&self) -> &'static str {
        T::persistent_name()
    }

    fn title(&self, cx: &App) -> SharedString {
        self.read(cx).title(cx)
    }

    fn icon(&self, cx: &App) -> PanelIcon {
        self.read(cx).icon()
    }

    fn position(&self, cx: &App) -> DockPosition {
        self.read(cx).position(cx)
    }

    fn position_is_valid(&self, position: DockPosition, cx: &App) -> bool {
        self.read(cx).position_is_valid(position)
    }

    fn set_position(&self, position: DockPosition, window: &mut Window, cx: &mut App) {
        self.update(cx, |this, cx| this.set_position(position, window, cx))
    }

    fn default_size(&self, cx: &App) -> Pixels {
        self.read(cx).default_size(cx)
    }

    fn min_size(&self, cx: &App) -> Option<Pixels> {
        self.read(cx).min_size()
    }

    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }

    fn to_any(&self) -> AnyView {
        self.clone().into()
    }
}

/// Type-erased, cheaply cloned handle to a registered panel.
///
/// signatures readable.
pub type AnyPanelHandle = Arc<dyn PanelHandle>;

/// Builds a fresh panel view. Stored in [`PanelRegistry`].
pub type PanelConstructor = Arc<dyn Fn(&mut Window, &mut App) -> AnyPanelHandle + Send + Sync>;

/// One panel type's registration record.
pub struct PanelRegistration {
    /// [`Panel::persistent_name`] of the registered type.
    pub persistent_name: &'static str,
    /// Dock the panel opens in before the user moves it.
    pub default_position: DockPosition,
    /// Status-bar toggle glyph.
    pub icon: PanelIcon,
    /// Constructor invoked lazily when the panel is first shown.
    pub build: PanelConstructor,
}

/// The set of every panel type known to the running app.
///
/// Labonair's architecture (`docs/architecture.md` §1.3, §4) wants one panel
/// registry read by both the workspace (which dock renders what) and the
/// status bar (one toggle per registration). Capability modules build typed
/// contributions; application composition only inserts them into this list.
#[derive(Default)]
pub struct PanelRegistry {
    panels: Vec<PanelRegistration>,
}

impl PanelRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a panel type. A later registration with the same
    /// `persistent_name` replaces the earlier one.
    pub fn register(&mut self, registration: PanelRegistration) {
        if let Some(slot) = self
            .panels
            .iter_mut()
            .find(|p| p.persistent_name == registration.persistent_name)
        {
            *slot = registration;
        } else {
            self.panels.push(registration);
        }
    }

    /// All registrations, in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &PanelRegistration> {
        self.panels.iter()
    }

    /// Registrations whose `default_position` equals `position`.
    pub fn for_position(&self, position: DockPosition) -> impl Iterator<Item = &PanelRegistration> {
        self.panels
            .iter()
            .filter(move |p| p.default_position == position)
    }

    /// Look up a registration by [`Panel::persistent_name`].
    pub fn get(&self, persistent_name: &str) -> Option<&PanelRegistration> {
        self.panels
            .iter()
            .find(|p| p.persistent_name == persistent_name)
    }

    /// Number of registered panel types.
    pub fn len(&self) -> usize {
        self.panels.len()
    }

    /// Whether no panel type is registered.
    pub fn is_empty(&self) -> bool {
        self.panels.is_empty()
    }
}

impl Global for PanelRegistry {}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_panel(
        name: &'static str,
        position: DockPosition,
        icon: PanelIcon,
    ) -> PanelRegistration {
        PanelRegistration {
            persistent_name: name,
            default_position: position,
            icon,
            // Never invoked by the registry's bookkeeping methods.
            build: Arc::new(|_, _| unreachable!("stub constructor")),
        }
    }

    #[test]
    fn dock_position_next_wraps() {
        assert_eq!(DockPosition::Left.next(), DockPosition::Right);
        assert_eq!(DockPosition::Right.next(), DockPosition::Bottom);
        assert_eq!(DockPosition::Bottom.next(), DockPosition::Left);
        assert_eq!(DockPosition::ALL.len(), 3);
    }

    #[test]
    fn register_and_lookup() {
        let mut reg = PanelRegistry::new();
        assert!(reg.is_empty());
        reg.register(stub_panel(
            "explorer",
            DockPosition::Left,
            PanelIcon::Explorer,
        ));
        reg.register(stub_panel("ai", DockPosition::Right, PanelIcon::Ai));
        assert_eq!(reg.len(), 2);
        assert!(reg.get("explorer").is_some());
        assert!(reg.get("missing").is_none());
        let names: Vec<_> = reg.iter().map(|p| p.persistent_name).collect();
        assert_eq!(names, ["explorer", "ai"]);
    }

    #[test]
    fn register_replaces_duplicate_name() {
        let mut reg = PanelRegistry::new();
        reg.register(stub_panel(
            "scm",
            DockPosition::Left,
            PanelIcon::SourceControl,
        ));
        reg.register(stub_panel(
            "scm",
            DockPosition::Right,
            PanelIcon::SourceControl,
        ));
        assert_eq!(reg.len(), 1);
        assert_eq!(
            reg.get("scm").unwrap().default_position,
            DockPosition::Right
        );
    }

    #[test]
    fn for_position_filters() {
        let mut reg = PanelRegistry::new();
        reg.register(stub_panel(
            "explorer",
            DockPosition::Left,
            PanelIcon::Explorer,
        ));
        reg.register(stub_panel(
            "scm",
            DockPosition::Left,
            PanelIcon::SourceControl,
        ));
        reg.register(stub_panel("ai", DockPosition::Right, PanelIcon::Ai));
        let left: Vec<_> = reg
            .for_position(DockPosition::Left)
            .map(|p| p.persistent_name)
            .collect();
        assert_eq!(left, ["explorer", "scm"]);
        assert_eq!(reg.for_position(DockPosition::Bottom).count(), 0);
    }
}
