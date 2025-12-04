/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { localize } from '../../../../nls.js';
import { IAction, Separator, toAction } from '../../../../base/common/actions.js';
import { BaseActionViewItem, IBaseActionViewItemOptions } from '../../../../base/browser/ui/actionbar/actionViewItems.js';
import { $, addDisposableListener, append, EventHelper, EventType, getWindow } from '../../../../base/browser/dom.js';
import { IContextMenuService } from '../../../../platform/contextview/browser/contextView.js';
import { ICommandService } from '../../../../platform/commands/common/commands.js';
import { ThemeIcon } from '../../../../base/common/themables.js';
import { Codicon } from '../../../../base/common/codicons.js';
import { StandardMouseEvent } from '../../../../base/browser/mouseEvent.js';
import { AnchorAlignment, AnchorAxisAlignment } from '../../../../base/browser/ui/contextview/contextview.js';

export class SettingsSplitButtonActionViewItem extends BaseActionViewItem {

	private button!: HTMLElement;
	private dropdownButton!: HTMLElement;

	constructor(
		action: IAction,
		options: IBaseActionViewItemOptions,
		@IContextMenuService private readonly contextMenuService: IContextMenuService,
		@ICommandService private readonly commandService: ICommandService
	) {
		super(null, action, options);
	}

	override render(container: HTMLElement): void {
		super.render(container);

		if (!this.element) {
			return;
		}

		this.element.classList.add('action-item', 'settings-split-button');

		// Main button (opens settings directly)
		this.button = append(this.element, $('a.action-label.settings-button'));
		this.button.tabIndex = 0;
		this.button.setAttribute('role', 'button');
		this.button.setAttribute('aria-label', localize('settings', "Settings"));
		this.button.title = localize('openSettings', "Open Settings");

		// Add gear icon
		const icon = append(this.button, $('span.codicon'));
		icon.classList.add(...ThemeIcon.asClassNameArray(Codicon.settingsGear));

		// Dropdown button (shows menu)
		this.dropdownButton = append(this.element, $('a.action-label.dropdown-button'));
		this.dropdownButton.tabIndex = 0;
		this.dropdownButton.setAttribute('role', 'button');
		this.dropdownButton.setAttribute('aria-label', localize('settingsMenu', "Settings Menu"));
		this.dropdownButton.title = localize('showSettingsMenu', "Show Settings Menu");

		// Add chevron icon
		const chevron = append(this.dropdownButton, $('span.codicon'));
		chevron.classList.add(...ThemeIcon.asClassNameArray(Codicon.chevronDown));

		this.registerListeners();
	}

	private registerListeners(): void {
		// Main button click - open settings
		this._register(addDisposableListener(this.button, EventType.CLICK, (e: MouseEvent) => {
			EventHelper.stop(e, true);
			this.openSettings();
		}));

		// Dropdown button click - show menu
		this._register(addDisposableListener(this.dropdownButton, EventType.CLICK, (e: MouseEvent) => {
			EventHelper.stop(e, true);
			this.showMenu(e);
		}));

		// Keyboard support for main button
		this._register(addDisposableListener(this.button, EventType.KEY_DOWN, (e: KeyboardEvent) => {
			if (e.key === 'Enter' || e.key === ' ') {
				EventHelper.stop(e, true);
				this.openSettings();
			}
		}));

		// Keyboard support for dropdown button
		this._register(addDisposableListener(this.dropdownButton, EventType.KEY_DOWN, (e: KeyboardEvent) => {
			if (e.key === 'Enter' || e.key === ' ') {
				EventHelper.stop(e, true);
				this.showMenu(e);
			}
		}));
	}

	private openSettings(): void {
		this.commandService.executeCommand('workbench.action.openSettings');
	}

	private showMenu(event: MouseEvent | KeyboardEvent): void {
		const actions = this.getMenuActions();

		const mouseEvent = event instanceof MouseEvent ? new StandardMouseEvent(getWindow(this.dropdownButton), event) : undefined;

		this.contextMenuService.showContextMenu({
			getAnchor: () => mouseEvent ? mouseEvent : this.dropdownButton,
			getActions: () => actions,
			anchorAlignment: AnchorAlignment.RIGHT,
			anchorAxisAlignment: AnchorAxisAlignment.VERTICAL
		});
	}

	private getMenuActions(): IAction[] {
		return [
			toAction({
				id: 'workbench.action.openSettings',
				label: localize('openSettings', "Settings"),
				run: () => this.commandService.executeCommand('workbench.action.openSettings')
			}),
			toAction({
				id: 'workbench.action.openGlobalKeybindings',
				label: localize('keyboardShortcuts', "Keyboard Shortcuts"),
				run: () => this.commandService.executeCommand('workbench.action.openGlobalKeybindings')
			}),
			toAction({
				id: 'workbench.action.openSnippets',
				label: localize('userSnippets', "User Snippets"),
				run: () => this.commandService.executeCommand('workbench.action.openSnippets')
			}),
			new Separator(),
			toAction({
				id: 'workbench.extensions.action.showInstalledExtensions',
				label: localize('extensions', "Extensions"),
				run: () => this.commandService.executeCommand('workbench.extensions.action.showInstalledExtensions')
			}),
			toAction({
				id: 'workbench.action.openThemeStudio',
				label: localize('themeStudio', "Theme Studio"),
				run: () => this.commandService.executeCommand('workbench.action.openThemeStudio')
			})
		];
	}

	override focus(): void {
		this.button?.focus();
	}

	override blur(): void {
		this.button?.blur();
		this.dropdownButton?.blur();
	}
}
