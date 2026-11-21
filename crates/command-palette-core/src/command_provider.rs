// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Command-palette-owned command metadata.

use crate::{CommandDescriptor, CommandIcon, CommandId, CommandProvider};

#[derive(Clone, Copy, Debug, Default)]
pub struct CommandPaletteCommandProvider;

impl CommandProvider for CommandPaletteCommandProvider {
    fn commands(&self) -> Vec<CommandDescriptor> {
        vec![CommandDescriptor::new(
            CommandId::OpenCommandPalette,
            "Open Command Palette",
            "Application",
        )
        .with_default_binding("cmd-p", None)
        .with_icon(CommandIcon::Command)]
    }
}
