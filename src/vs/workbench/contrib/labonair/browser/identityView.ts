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
import { IIdentityService, IIdentity, IdentityType } from '../common/identityService.js';
import { $, addDisposableListener } from '../../../../base/browser/dom.js';
import { generateUuid } from '../../../../base/common/uuid.js';
import { INotificationService } from '../../../../platform/notification/common/notification.js';
import { IFileDialogService } from '../../../../platform/dialogs/common/dialogs.js';

enum FormMode {
	Hidden,
	Add,
	Edit
}

export class LabonairIdentityView extends ViewPane {
	private _container?: HTMLElement;
	private _listContainer?: HTMLElement;
	private _formContainer?: HTMLElement;

	private _formMode: FormMode = FormMode.Hidden;
	private _editingIdentity: IIdentity | null = null;

	constructor(
		options: IViewPaneOptions,
		@IKeybindingService keybindingService: IKeybindingService,
		@IContextMenuService contextMenuService: IContextMenuService,
		@IConfigurationService configurationService: IConfigurationService,
		@IContextKeyService contextKeyService: IContextKeyService,
		@IViewDescriptorService viewDescriptorService: IViewDescriptorService,
		@IInstantiationService private readonly instantiationService: IInstantiationService,
		@IOpenerService openerService: IOpenerService,
		@IThemeService themeService: IThemeService,
		@IHoverService hoverService: IHoverService,
		@IIdentityService private readonly identityService: IIdentityService,
		@INotificationService private readonly notificationService: INotificationService,
		@IFileDialogService private readonly fileDialogService: IFileDialogService
	) {
		super(options, keybindingService, contextMenuService, configurationService, contextKeyService, viewDescriptorService, instantiationService, openerService, themeService, hoverService);

		this._register(this.identityService.onDidChangeIdentities(() => {
			this._loadIdentities();
		}));
	}

	protected override renderBody(container: HTMLElement): void {
		super.renderBody(container);

		this._container = container;
		this._container.classList.add('labonair-identity-view');

		// Create list container
		this._listContainer = $('div.list-container');
		this._container.appendChild(this._listContainer);

		// Create form container
		this._formContainer = $('div.identity-form-overlay.hidden');
		this._container.appendChild(this._formContainer);

		this._renderList();
		this._renderForm();
	}

	private _renderList(): void {
		if (!this._listContainer) {
			return;
		}

		const html = `
			<div class="identity-manager">
				<div class="header">
					<h2 class="view-title">Identity Manager</h2>
					<button class="add-identity-button">Add Identity</button>
				</div>
				<div class="identity-list" id="identityList">
					<div class="loading">Loading identities...</div>
				</div>
			</div>
		`;

		this._listContainer.innerHTML = html;

		// Add event listeners
		const addButton = this._listContainer.querySelector('.add-identity-button');
		if (addButton) {
			this._register(addDisposableListener(addButton, 'click', () => this._showForm(FormMode.Add)));
		}

		this._loadIdentities();
	}

	private _renderForm(): void {
		if (!this._formContainer) {
			return;
		}

		this._formContainer.innerHTML = `
			<div class="identity-form-header">
				<h2 class="identity-form-title" id="formTitle">Add Identity</h2>
			</div>
			<div class="identity-form-content">
				<!-- Meta Information -->
				<div class="form-group">
					<div class="form-row">
						<div class="form-field">
							<label class="form-label required">Name</label>
							<input type="text" class="form-input" id="identityName" placeholder="Company Prod Key" />
							<div class="form-error" id="identityNameError"></div>
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Type</label>
							<div class="segmented-control">
								<button class="segmented-control-item active" data-type="ssh-key">SSH Key</button>
								<button class="segmented-control-item" data-type="password">Password</button>
							</div>
						</div>
					</div>
				</div>

				<!-- SSH Key Fields -->
				<div class="form-group identity-type-field" id="sshKeyFields">
					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Private Key</label>
							<div class="file-upload-area" id="keyUploadArea">
								<div class="file-upload-text">Click to upload or paste key content</div>
								<input type="file" class="file-upload-input" id="keyFileInput" accept=".pem,.key,.rsa,.ed25519" />
							</div>
							<textarea class="form-textarea" id="keyContent" placeholder="Paste private key here..." style="margin-top: 8px; font-family: monospace;" rows="8"></textarea>
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Passphrase (Optional)</label>
							<input type="password" class="form-input" id="keyPassphrase" placeholder="Enter passphrase if key is encrypted" />
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Comment (Optional)</label>
							<input type="text" class="form-input" id="keyComment" placeholder="e.g., Generated for production servers" />
						</div>
					</div>
				</div>

				<!-- Password Fields -->
				<div class="form-group identity-type-field hidden" id="passwordFields">
					<div class="form-row">
						<div class="form-field">
							<label class="form-label required">Password</label>
							<input type="password" class="form-input" id="identityPassword" placeholder="Enter password" />
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Confirm Password</label>
							<input type="password" class="form-input" id="identityPasswordConfirm" placeholder="Confirm password" />
						</div>
					</div>
				</div>
			</div>
			<div class="identity-form-footer">
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

		// Type switching
		const typeButtons = this._formContainer.querySelectorAll('[data-type]');
		typeButtons.forEach(btn => {
			this._register(addDisposableListener(btn, 'click', () => {
				this._handleTypeChange(btn as HTMLElement);
			}));
		});

		// Key file upload
		const keyUploadArea = this._formContainer.querySelector('#keyUploadArea');
		const keyFileInput = this._formContainer.querySelector('#keyFileInput') as HTMLInputElement;
		if (keyUploadArea && keyFileInput) {
			this._register(addDisposableListener(keyUploadArea, 'click', () => keyFileInput.click()));
			this._register(addDisposableListener(keyFileInput, 'change', () => {
				const file = keyFileInput.files?.[0];
				if (file) {
					const reader = new FileReader();
					reader.onload = (e) => {
						const keyContent = this._formContainer?.querySelector('#keyContent') as HTMLTextAreaElement;
						if (keyContent) {
							keyContent.value = e.target?.result as string;
						}
					};
					reader.readAsText(file);
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
			this._register(addDisposableListener(saveBtn, 'click', () => this._saveIdentity()));
		}
	}

	private _handleTypeChange(button: HTMLElement): void {
		if (!this._formContainer) {
			return;
		}

		// Update button states
		const typeButtons = this._formContainer.querySelectorAll('[data-type]');
		typeButtons.forEach(btn => btn.classList.remove('active'));
		button.classList.add('active');

		// Show/hide fields
		const type = button.dataset['type'] as IdentityType;
		const sshKeyFields = this._formContainer.querySelector('#sshKeyFields');
		const passwordFields = this._formContainer.querySelector('#passwordFields');

		if (type === 'ssh-key') {
			sshKeyFields?.classList.remove('hidden');
			passwordFields?.classList.add('hidden');
		} else {
			sshKeyFields?.classList.add('hidden');
			passwordFields?.classList.remove('hidden');
		}
	}

	private _showForm(mode: FormMode, identity?: IIdentity): void {
		this._formMode = mode;
		this._editingIdentity = identity || null;

		if (!this._formContainer) {
			return;
		}

		// Update title
		const title = this._formContainer.querySelector('#formTitle');
		if (title) {
			title.textContent = mode === FormMode.Add ? 'Add Identity' : 'Edit Identity';
		}

		// Populate form if editing
		if (identity) {
			this._populateForm(identity);
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
		this._editingIdentity = null;

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
			if (input instanceof HTMLInputElement) {
				if (input.type === 'checkbox') {
					input.checked = false;
				} else {
					input.value = '';
				}
			} else if (input instanceof HTMLTextAreaElement) {
				input.value = '';
			}
		});

		// Reset to SSH key type
		const sshKeyButton = this._formContainer.querySelector('[data-type="ssh-key"]');
		if (sshKeyButton) {
			this._handleTypeChange(sshKeyButton as HTMLElement);
		}
	}

	private _populateForm(identity: IIdentity): void {
		if (!this._formContainer) {
			return;
		}

		// Name
		(this._formContainer.querySelector('#identityName') as HTMLInputElement).value = identity.name;

		// Type
		const typeBtn = this._formContainer.querySelector(`[data-type="${identity.type}"]`);
		if (typeBtn) {
			this._handleTypeChange(typeBtn as HTMLElement);
		}

		// Comment
		if (identity.comment) {
			(this._formContainer.querySelector('#keyComment') as HTMLInputElement).value = identity.comment;
		}

		// Note: We don't load private data for security reasons
	}

	private async _saveIdentity(): Promise<void> {
		if (!this._formContainer) {
			return;
		}

		// Validate
		if (!this._validateForm()) {
			return;
		}

		// Collect form data
		const name = (this._formContainer.querySelector('#identityName') as HTMLInputElement).value;
		const activeTypeBtn = this._formContainer.querySelector('[data-type].active');
		const type = (activeTypeBtn as HTMLElement)?.dataset['type'] as IdentityType || 'ssh-key';

		let privateData = '';
		let passphrase: string | undefined;

		if (type === 'ssh-key') {
			const keyContent = (this._formContainer.querySelector('#keyContent') as HTMLTextAreaElement).value;
			const keyPassphrase = (this._formContainer.querySelector('#keyPassphrase') as HTMLInputElement).value;
			const keyComment = (this._formContainer.querySelector('#keyComment') as HTMLInputElement).value;

			privateData = keyContent;
			passphrase = keyPassphrase || undefined;

			const identity: IIdentity = {
				id: this._editingIdentity?.id || generateUuid(),
				name,
				type: 'ssh-key',
				comment: keyComment || undefined,
				createdAt: this._editingIdentity?.createdAt || Date.now()
			};

			try {
				if (this._formMode === FormMode.Edit && this._editingIdentity) {
					await this.identityService.updateIdentity(this._editingIdentity.id, identity);
					// Only update private data if it was changed
					if (keyContent) {
						await this.identityService.addIdentity(identity, privateData, passphrase);
					}
				} else {
					await this.identityService.addIdentity(identity, privateData, passphrase);
				}

				this.notificationService.info(`Identity "${name}" saved successfully`);
				this._hideForm();
				await this._loadIdentities();
			} catch (error) {
				this.notificationService.error(`Failed to save identity: ${error instanceof Error ? error.message : String(error)}`);
			}
		} else {
			// Password type
			const password = (this._formContainer.querySelector('#identityPassword') as HTMLInputElement).value;
			const passwordConfirm = (this._formContainer.querySelector('#identityPasswordConfirm') as HTMLInputElement).value;

			if (password !== passwordConfirm) {
				this.notificationService.error('Passwords do not match');
				return;
			}

			privateData = password;

			const identity: IIdentity = {
				id: this._editingIdentity?.id || generateUuid(),
				name,
				type: 'password',
				createdAt: this._editingIdentity?.createdAt || Date.now()
			};

			try {
				if (this._formMode === FormMode.Edit && this._editingIdentity) {
					await this.identityService.updateIdentity(this._editingIdentity.id, identity);
					// Only update private data if it was changed
					if (password) {
						await this.identityService.addIdentity(identity, privateData);
					}
				} else {
					await this.identityService.addIdentity(identity, privateData);
				}

				this.notificationService.info(`Identity "${name}" saved successfully`);
				this._hideForm();
				await this._loadIdentities();
			} catch (error) {
				this.notificationService.error(`Failed to save identity: ${error instanceof Error ? error.message : String(error)}`);
			}
		}
	}

	private _validateForm(): boolean {
		if (!this._formContainer) {
			return false;
		}

		let isValid = true;

		// Validate name
		const nameInput = this._formContainer.querySelector('#identityName') as HTMLInputElement;
		const nameError = this._formContainer.querySelector('#identityNameError');
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

		// Validate based on type
		const activeTypeBtn = this._formContainer.querySelector('[data-type].active');
		const type = (activeTypeBtn as HTMLElement)?.dataset['type'] as IdentityType || 'ssh-key';

		if (type === 'ssh-key') {
			const keyContent = (this._formContainer.querySelector('#keyContent') as HTMLTextAreaElement)?.value;
			if (!keyContent?.trim() && this._formMode === FormMode.Add) {
				this.notificationService.error('Private key content is required');
				isValid = false;
			}
		} else {
			const password = (this._formContainer.querySelector('#identityPassword') as HTMLInputElement)?.value;
			if (!password && this._formMode === FormMode.Add) {
				this.notificationService.error('Password is required');
				isValid = false;
			}
		}

		return isValid;
	}

	private async _loadIdentities(): Promise<void> {
		const identities = await this.identityService.getAllIdentities();
		const identityListElement = this._listContainer?.querySelector('#identityList');

		if (!identityListElement) {
			return;
		}

		if (identities.length === 0) {
			identityListElement.innerHTML = '<div class="empty-state">No identities configured. Click "Add Identity" to get started.</div>';
			return;
		}

		const identitiesHtml = identities.map(identity => `
			<div class="identity-card" data-identity-id="${identity.id}">
				<div class="identity-header">
					<span class="identity-icon">${identity.type === 'ssh-key' ? '🔑' : '🔒'}</span>
					<span class="identity-name">${identity.name}</span>
					<span class="identity-type">${identity.type === 'ssh-key' ? 'SSH Key' : 'Password'}</span>
				</div>
				${identity.comment ? `<div class="identity-comment">${identity.comment}</div>` : ''}
				<div class="identity-actions">
					<button class="identity-edit-btn" data-identity-id="${identity.id}">Edit</button>
					<button class="identity-delete-btn" data-identity-id="${identity.id}">Delete</button>
				</div>
			</div>
		`).join('');

		identityListElement.innerHTML = identitiesHtml;

		// Add click handlers
		const editButtons = identityListElement.querySelectorAll('.identity-edit-btn');
		editButtons.forEach(btn => {
			this._register(addDisposableListener(btn, 'click', async () => {
				const identityId = (btn as HTMLElement).dataset['identityId'];
				if (identityId) {
					const identity = await this.identityService.getIdentity(identityId);
					if (identity) {
						this._showForm(FormMode.Edit, identity);
					}
				}
			}));
		});

		const deleteButtons = identityListElement.querySelectorAll('.identity-delete-btn');
		deleteButtons.forEach(btn => {
			this._register(addDisposableListener(btn, 'click', async () => {
				const identityId = (btn as HTMLElement).dataset['identityId'];
				if (identityId) {
					const identity = await this.identityService.getIdentity(identityId);
					if (identity && confirm(`Are you sure you want to delete the identity "${identity.name}"?`)) {
						await this.identityService.deleteIdentity(identityId);
						this.notificationService.info(`Identity "${identity.name}" deleted`);
						await this._loadIdentities();
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
