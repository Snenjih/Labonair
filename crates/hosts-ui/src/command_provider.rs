// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Hosts-UI-owned command execution contributions.

use std::rc::Rc;

use gpui::{App, Window};
use labonair_command_palette_core::{CommandId, PaletteAction, SubmenuAction};
use labonair_command_palette_runtime::CommandHandlerRegistry;
use labonair_command_palette_runtime::PaletteActionHandlerRegistry;
use labonair_hosts::HostOpenRequest;

/// Host operation used to open the canonical command-palette host picker.
pub type HostPickerHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Composition callback for opening a Hosts-owned connection request in the
/// active workspace. Hosts-UI owns action decoding; Workspace owns tab state.
pub type HostOpenHandler = Rc<dyn Fn(HostOpenRequest, &mut Window, &mut App)>;

/// Register connection commands that all enter through the Hosts picker.
/// `Enter` and `Shift+Enter` then select SSH or SFTP on the picker row.
pub fn register_picker_handlers(
    registry: &mut CommandHandlerRegistry,
    open_picker: HostPickerHandler,
) {
    for id in [CommandId::NewSshTab, CommandId::NewSftpTab] {
        let open_picker = open_picker.clone();
        registry
            .register(id, move |window, cx| open_picker(window, cx))
            .expect("hosts picker command handler must have a unique id");
    }
}

/// Register the dynamic Hosts submenu action. The injected callback is the
/// narrow workspace boundary; no shell feature match is required.
pub fn register_palette_action_handlers(
    registry: &mut PaletteActionHandlerRegistry,
    open_host: HostOpenHandler,
) {
    registry
        .register("hosts", move |action, window, cx| {
            let PaletteAction::Submenu(SubmenuAction::ConnectHost { host_id, sftp }) = action
            else {
                return false;
            };
            let request = if *sftp {
                HostOpenRequest::sftp(host_id.clone())
            } else {
                HostOpenRequest::ssh(host_id.clone())
            };
            open_host(request, window, cx);
            true
        })
        .expect("hosts palette action handler must have a unique owner");
}
