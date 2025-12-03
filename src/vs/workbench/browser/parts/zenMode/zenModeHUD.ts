/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Disposable, DisposableStore } from '../../../../base/common/lifecycle.js';
import { $, addDisposableListener, append, EventType, hide, show } from '../../../../base/browser/dom.js';
import { IEditorService } from '../../../services/editor/common/editorService.js';
import { IWorkspaceContextService } from '../../../../platform/workspace/common/workspace.js';
import './media/zenModeHUD.css';

export interface IZenModeHUDOptions {
	showHUD: boolean;
	hudTimeout: number;
}

/**
 * Zen Mode 2.0 HUD Component
 *
 * Displays a transparent HUD overlay at the top of the screen during Zen Mode showing:
 * - Current file/project name
 * - Git branch (if available)
 * - Current time
 * - Exit hint
 */
export class ZenModeHUD extends Disposable {

	private container: HTMLElement;
	private hudElement: HTMLElement | undefined;
	private timeoutHandle: any;
	private isVisible: boolean = false;
	private readonly mouseListenerDisposables = this._register(new DisposableStore());

	constructor(
		container: HTMLElement,
		private readonly options: IZenModeHUDOptions,
		@IEditorService private readonly editorService: IEditorService,
		@IWorkspaceContextService private readonly workspaceContextService: IWorkspaceContextService
	) {
		super();
		this.container = container;
	}

	show(): void {
		if (!this.options.showHUD) {
			return;
		}

		if (!this.hudElement) {
			this.create();
		}

		if (this.hudElement) {
			this.update();
			show(this.hudElement);
			this.isVisible = true;
			this.scheduleAutoHide();
			this.registerMouseListeners();
		}
	}

	hide(): void {
		if (this.hudElement) {
			hide(this.hudElement);
			this.isVisible = false;
			this.clearAutoHideTimeout();
			this.mouseListenerDisposables.clear();
		}
	}

	update(): void {
		if (!this.hudElement) {
			return;
		}

		const fileNameElement = this.hudElement.querySelector('.zen-hud-filename');
		const projectElement = this.hudElement.querySelector('.zen-hud-project');
		const timeElement = this.hudElement.querySelector('.zen-hud-time');

		// Update filename
		const activeEditor = this.editorService.activeEditor;
		if (fileNameElement) {
			fileNameElement.textContent = activeEditor?.getName() || 'No file open';
		}

		// Update project name
		const workspace = this.workspaceContextService.getWorkspace();
		if (projectElement) {
			if (workspace.folders.length > 0) {
				projectElement.textContent = workspace.folders[0].name;
			} else {
				projectElement.textContent = 'No workspace';
			}
		}

		// Update time
		if (timeElement) {
			const now = new Date();
			timeElement.textContent = now.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
		}
	}

	private create(): void {
		this.hudElement = append(this.container, $('.zen-mode-hud'));

		const hudContent = append(this.hudElement, $('.zen-hud-content'));

		// File/Project info section
		const infoSection = append(hudContent, $('.zen-hud-info'));
		append(infoSection, $('.zen-hud-filename'));
		append(infoSection, $('span.zen-hud-separator', {}, '•'));
		append(infoSection, $('.zen-hud-project'));

		// Time section
		append(hudContent, $('.zen-hud-time'));

		// Exit hint
		const hintElement = append(hudContent, $('.zen-hud-hint'));
		hintElement.textContent = 'Press Esc to exit Zen Mode';

		hide(this.hudElement);
	}

	private scheduleAutoHide(): void {
		this.clearAutoHideTimeout();
		if (this.options.hudTimeout > 0) {
			this.timeoutHandle = setTimeout(() => {
				this.hide();
			}, this.options.hudTimeout * 1000);
		}
	}

	private clearAutoHideTimeout(): void {
		if (this.timeoutHandle) {
			clearTimeout(this.timeoutHandle);
			this.timeoutHandle = undefined;
		}
	}

	private registerMouseListeners(): void {
		this.mouseListenerDisposables.clear();

		// Show HUD on mouse movement near top of screen
		this.mouseListenerDisposables.add(addDisposableListener(this.container, EventType.MOUSE_MOVE, (e: MouseEvent) => {
			if (e.clientY < 100 && !this.isVisible) {
				this.show();
			}
		}));
	}

	override dispose(): void {
		this.clearAutoHideTimeout();
		this.hudElement?.remove();
		super.dispose();
	}
}
