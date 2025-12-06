/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { IViewPaneOptions, ViewPane } from '../../../browser/parts/views/viewPane.js';
import { IKeybindingService } from '../../../../platform/keybinding/common/keybinding.js';
import { IContextMenuService } from '../../../../platform/contextview/browser/contextView.js';
import { IConfigurationService } from '../../../../platform/configuration/common/configuration.js';
import { IContextKeyService } from '../../../../platform/contextkey/common/contextkey.js';
import { IViewDescriptorService } from '../../../common/views.js';
import { IInstantiationService } from '../../../../platform/instantiation/common/instantiation.js';
import { IOpenerService } from '../../../../platform/opener/common/opener.js';
import { IThemeService } from '../../../../platform/theme/common/themeService.js';
import { IHoverService } from '../../../../platform/hover/browser/hover.js';
import { IScriptService, IScript } from '../common/scriptService.js';
import { $, addDisposableListener } from '../../../../base/browser/dom.js';
import { generateUuid } from '../../../../base/common/uuid.js';
import { INotificationService } from '../../../../platform/notification/common/notification.js';

enum FormMode {
	Hidden,
	Add,
	Edit
}

export class LabonairScriptView extends ViewPane {
	private _container?: HTMLElement;
	private _listContainer?: HTMLElement;
	private _formContainer?: HTMLElement;

	private _formMode: FormMode = FormMode.Hidden;
	private _editingScript: IScript | null = null;
	private _tags: string[] = [];

	constructor(
		options: IViewPaneOptions,
		@IKeybindingService keybindingService: IKeybindingService,
		@IContextMenuService contextMenuService: IContextMenuService,
		@IConfigurationService configurationService: IConfigurationService,
		@IContextKeyService contextKeyService: IContextKeyService,
		@IViewDescriptorService viewDescriptorService: IViewDescriptorService,
		@IInstantiationService override readonly instantiationService: IInstantiationService,
		@IOpenerService openerService: IOpenerService,
		@IThemeService themeService: IThemeService,
		@IHoverService hoverService: IHoverService,
		@IScriptService private readonly scriptService: IScriptService,
		@INotificationService private readonly notificationService: INotificationService
	) {
		super(options, keybindingService, contextMenuService, configurationService, contextKeyService, viewDescriptorService, instantiationService, openerService, themeService, hoverService);

		this._register(this.scriptService.onDidChangeScripts(() => {
			this._loadScripts();
		}));
	}

	protected override renderBody(container: HTMLElement): void {
		super.renderBody(container);

		this._container = container;
		this._container.classList.add('labonair-script-view');

		// Create list container
		this._listContainer = $('div.list-container');
		this._container.appendChild(this._listContainer);

		// Create form container
		this._formContainer = $('div.script-form-overlay.hidden');
		this._container.appendChild(this._formContainer);

		this._renderList();
		this._renderForm();
	}

	private _renderList(): void {
		if (!this._listContainer) {
			return;
		}

		const html = `
			<div class="script-manager">
				<div class="header">
					<h2 class="view-title">Scripts & Snippets</h2>
					<button class="add-script-button">Add Script</button>
				</div>
				<div class="script-list" id="scriptList">
					<div class="loading">Loading scripts...</div>
				</div>
			</div>
		`;

		this._listContainer.innerHTML = html;

		// Add event listeners
		const addButton = this._listContainer.querySelector('.add-script-button');
		if (addButton) {
			this._register(addDisposableListener(addButton, 'click', () => this._showForm(FormMode.Add)));
		}

		this._loadScripts();
	}

	private _renderForm(): void {
		if (!this._formContainer) {
			return;
		}

		this._formContainer.innerHTML = `
			<div class="script-form-header">
				<h2 class="script-form-title" id="formTitle">Add Script</h2>
			</div>
			<div class="script-form-content">
				<!-- Basic Information -->
				<div class="form-group">
					<div class="form-row">
						<div class="form-field">
							<label class="form-label required">Name</label>
							<input type="text" class="form-input" id="scriptName" placeholder="Restart Docker" />
							<div class="form-error" id="scriptNameError"></div>
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Description</label>
							<input type="text" class="form-input" id="scriptDescription" placeholder="Restarts Docker daemon and cleans up containers" />
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Tags</label>
							<div class="tag-input-container" id="tagContainer">
								<input type="text" class="tag-input" id="tagInput" placeholder="Add tag..." />
							</div>
						</div>
					</div>
				</div>

				<!-- Script Content -->
				<div class="form-group">
					<div class="form-row">
						<div class="form-field">
							<label class="form-label required">Script Content</label>
							<textarea class="form-textarea" id="scriptContent" placeholder="#!/bin/bash&#10;systemctl restart docker&#10;docker system prune -f" rows="12" style="font-family: monospace;"></textarea>
							<div class="form-error" id="scriptContentError"></div>
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px; display: block; margin-top: 4px;">
								Enter shell commands to execute. Each line will be run sequentially.
							</small>
						</div>
					</div>
				</div>
			</div>
			<div class="script-form-footer">
				<button class="form-btn form-btn-cancel" id="formCancelBtn">Cancel</button>
				<button class="form-btn form-btn-save" id="formSaveBtn">Save</button>
			</div>
		`;

		this._attachFormEventListeners();
	}

	private _attachFormEventListeners(): void {
		if (!this._formContainer) {
			return;
		}

		// Tag input
		const tagInput = this._formContainer.querySelector('#tagInput') as HTMLInputElement;
		if (tagInput) {
			this._register(addDisposableListener(tagInput, 'keydown', (e: KeyboardEvent) => {
				if (e.key === 'Enter') {
					e.preventDefault();
					this._addTag(tagInput.value.trim());
					tagInput.value = '';
				}
			}));
		}

		// Cancel button
		const cancelBtn = this._formContainer.querySelector('#formCancelBtn');
		if (cancelBtn) {
			this._register(addDisposableListener(cancelBtn, 'click', () => this._hideForm()));
		}

		// Save button
		const saveBtn = this._formContainer.querySelector('#formSaveBtn');
		if (saveBtn) {
			this._register(addDisposableListener(saveBtn, 'click', () => this._saveScript()));
		}
	}

	private _addTag(tag: string): void {
		if (!tag || this._tags.includes(tag)) {
			return;
		}

		this._tags.push(tag);
		this._renderTags();
	}

	private _removeTag(tag: string): void {
		this._tags = this._tags.filter(t => t !== tag);
		this._renderTags();
	}

	private _renderTags(): void {
		const tagContainer = this._formContainer?.querySelector('#tagContainer');
		const tagInput = this._formContainer?.querySelector('#tagInput') as HTMLInputElement;

		if (!tagContainer || !tagInput) {
			return;
		}

		// Clear existing tags
		const existingTags = tagContainer.querySelectorAll('.tag-pill');
		existingTags.forEach(tag => tag.remove());

		// Render tags
		this._tags.forEach(tag => {
			const tagPill = document.createElement('div');
			tagPill.className = 'tag-pill';
			tagPill.innerHTML = `
				${tag}
				<span class="tag-pill-remove" data-tag="${tag}">×</span>
			`;

			const removeBtn = tagPill.querySelector('.tag-pill-remove');
			if (removeBtn) {
				this._register(addDisposableListener(removeBtn, 'click', () => {
					this._removeTag(tag);
				}));
			}

			tagContainer.insertBefore(tagPill, tagInput);
		});
	}

	private _showForm(mode: FormMode, script?: IScript): void {
		this._formMode = mode;
		this._editingScript = script || null;
		this._tags = script?.tags || [];

		if (!this._formContainer) {
			return;
		}

		// Update title
		const title = this._formContainer.querySelector('#formTitle');
		if (title) {
			title.textContent = mode === FormMode.Add ? 'Add Script' : 'Edit Script';
		}

		// Populate form if editing
		if (script) {
			this._populateForm(script);
		} else {
			this._resetForm();
		}

		// Show form
		this._formContainer.classList.remove('hidden');
		if (this._listContainer) {
			this._listContainer.style.display = 'none';
		}
	}

	private _hideForm(): void {
		this._formMode = FormMode.Hidden;
		this._editingScript = null;
		this._tags = [];

		if (this._formContainer) {
			this._formContainer.classList.add('hidden');
		}
		if (this._listContainer) {
			this._listContainer.style.display = 'block';
		}
	}

	private _resetForm(): void {
		if (!this._formContainer) {
			return;
		}

		// Reset all inputs
		const inputs = this._formContainer.querySelectorAll('input, textarea');
		inputs.forEach(input => {
			if (input instanceof HTMLInputElement || input instanceof HTMLTextAreaElement) {
				input.value = '';
			}
		});

		// Reset tags
		this._tags = [];
		this._renderTags();
	}

	private _populateForm(script: IScript): void {
		if (!this._formContainer) {
			return;
		}

		(this._formContainer.querySelector('#scriptName') as HTMLInputElement).value = script.name;
		(this._formContainer.querySelector('#scriptDescription') as HTMLInputElement).value = script.description || '';
		(this._formContainer.querySelector('#scriptContent') as HTMLTextAreaElement).value = script.content;

		this._tags = script.tags || [];
		this._renderTags();
	}

	private async _saveScript(): Promise<void> {
		if (!this._formContainer) {
			return;
		}

		// Validate
		if (!this._validateForm()) {
			return;
		}

		// Collect form data
		const name = (this._formContainer.querySelector('#scriptName') as HTMLInputElement).value;
		const description = (this._formContainer.querySelector('#scriptDescription') as HTMLInputElement).value;
		const content = (this._formContainer.querySelector('#scriptContent') as HTMLTextAreaElement).value;

		const script: IScript = {
			id: this._editingScript?.id || generateUuid(),
			name,
			description: description || undefined,
			content,
			tags: this._tags,
			createdAt: this._editingScript?.createdAt || Date.now()
		};

		try {
			if (this._formMode === FormMode.Edit && this._editingScript) {
				await this.scriptService.updateScript(this._editingScript.id, script);
			} else {
				await this.scriptService.addScript(script);
			}

			this.notificationService.info(`Script "${name}" saved successfully`);
			this._hideForm();
			await this._loadScripts();
		} catch (error) {
			this.notificationService.error(`Failed to save script: ${error instanceof Error ? error.message : String(error)}`);
		}
	}

	private _validateForm(): boolean {
		if (!this._formContainer) {
			return false;
		}

		let isValid = true;

		// Validate name
		const nameInput = this._formContainer.querySelector('#scriptName') as HTMLInputElement;
		const nameError = this._formContainer.querySelector('#scriptNameError');
		if (!nameInput.value.trim()) {
			nameInput.classList.add('invalid');
			if (nameError) {
				nameError.textContent = 'Name is required';
			}
			isValid = false;
		} else {
			nameInput.classList.remove('invalid');
			if (nameError) {
				nameError.textContent = '';
			}
		}

		// Validate content
		const contentInput = this._formContainer.querySelector('#scriptContent') as HTMLTextAreaElement;
		const contentError = this._formContainer.querySelector('#scriptContentError');
		if (!contentInput.value.trim()) {
			contentInput.classList.add('invalid');
			if (contentError) {
				contentError.textContent = 'Script content is required';
			}
			isValid = false;
		} else {
			contentInput.classList.remove('invalid');
			if (contentError) {
				contentError.textContent = '';
			}
		}

		return isValid;
	}

	private async _loadScripts(): Promise<void> {
		const scripts = await this.scriptService.getAllScripts();
		const scriptListElement = this._listContainer?.querySelector('#scriptList');

		if (!scriptListElement) {
			return;
		}

		if (scripts.length === 0) {
			scriptListElement.innerHTML = '<div class="empty-state">No scripts configured. Click "Add Script" to get started.</div>';
			return;
		}

		const scriptsHtml = scripts.map(script => `
			<div class="script-card" data-script-id="${script.id}" draggable="true">
				<div class="script-header">
					<span class="script-icon codicon codicon-terminal"></span>
					<span class="script-name">${script.name}</span>
				</div>
				${script.description ? `<div class="script-description">${script.description}</div>` : ''}
				<div class="script-tags">
					${script.tags?.map(tag => `<span class="tag">${tag}</span>`).join('') || ''}
				</div>
				<div class="script-actions">
					<button class="script-edit-btn" data-script-id="${script.id}">Edit</button>
					<button class="script-delete-btn" data-script-id="${script.id}">Delete</button>
				</div>
			</div>
		`).join('');

		scriptListElement.innerHTML = scriptsHtml;

		// Add drag & drop handlers
		const scriptCards = scriptListElement.querySelectorAll('.script-card');
		scriptCards.forEach(card => {
			this._register(addDisposableListener(card, 'dragstart', (e: DragEvent) => {
				const scriptId = (card as HTMLElement).dataset['scriptId'];
				if (scriptId && e.dataTransfer) {
					e.dataTransfer.effectAllowed = 'copy';
					e.dataTransfer.setData('text/plain', scriptId);
				}
			}));
		});

		// Add click handlers for edit buttons
		const editButtons = scriptListElement.querySelectorAll('.script-edit-btn');
		editButtons.forEach(btn => {
			this._register(addDisposableListener(btn, 'click', async () => {
				const scriptId = (btn as HTMLElement).dataset['scriptId'];
				if (scriptId) {
					const script = await this.scriptService.getScript(scriptId);
					if (script) {
						this._showForm(FormMode.Edit, script);
					}
				}
			}));
		});

		// Add click handlers for delete buttons
		const deleteButtons = scriptListElement.querySelectorAll('.script-delete-btn');
		deleteButtons.forEach(btn => {
			this._register(addDisposableListener(btn, 'click', async () => {
				const scriptId = (btn as HTMLElement).dataset['scriptId'];
				if (scriptId) {
					const script = await this.scriptService.getScript(scriptId);
					if (script && confirm(`Are you sure you want to delete the script "${script.name}"?`)) {
						await this.scriptService.deleteScript(scriptId);
						this.notificationService.info(`Script "${script.name}" deleted`);
						await this._loadScripts();
					}
				}
			}));
		});
	}

	protected override layoutBody(height: number, width: number): void {
		super.layoutBody(height, width);

		if (this._container) {
			this._container.style.height = `${height}px`;
			this._container.style.width = `${width}px`;
		}
	}
}
