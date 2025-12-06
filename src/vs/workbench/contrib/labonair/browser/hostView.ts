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
import { IHostService, IHost, AuthType, HostProtocol, OSIcon, IPortTunnel, HostStatus } from '../common/hostService.js';
import { IIdentityService } from '../common/identityService.js';
import { $, addDisposableListener } from '../../../../base/browser/dom.js';
import { generateUuid } from '../../../../base/common/uuid.js';
import { IQuickInputService, IQuickPickItem } from '../../../../platform/quickinput/common/quickInput.js';
import { IFileDialogService } from '../../../../platform/dialogs/common/dialogs.js';
import { INotificationService } from '../../../../platform/notification/common/notification.js';
import { URI } from '../../../../base/common/uri.js';
import { LabonairImporter, FileZillaImporter, WinSCPImporter, PuTTYImporter } from '../node/importers.js';
import { IEditorService } from '../../../services/editor/common/editorService.js';

enum FormMode {
	Hidden,
	Add,
	Edit
}

export class LabonairHostView extends ViewPane {
	private _container?: HTMLElement;
	private _listContainer?: HTMLElement;
	private _formContainer?: HTMLElement;

	private _formMode: FormMode = FormMode.Hidden;
	private _editingHost: IHost | null = null;
	private _tags: string[] = [];
	private _tunnels: IPortTunnel[] = [];
	private _activeSessions: Set<string> = new Set(); // Track active host sessions
	private _sortBy: 'alphabetical' | 'lastUsed' | 'status' = 'alphabetical'; // Current sort mode

	constructor(
		options: IViewPaneOptions,
		@IKeybindingService keybindingService: IKeybindingService,
		@IContextMenuService contextMenuService: IContextMenuService,
		@IConfigurationService configurationService: IConfigurationService,
		@IContextKeyService contextKeyService: IContextKeyService,
		@IViewDescriptorService viewDescriptorService: IViewDescriptorService,
		@IInstantiationService instantiationService: IInstantiationService,
		@IOpenerService openerService: IOpenerService,
		@IThemeService themeService: IThemeService,
		@IHoverService hoverService: IHoverService,
		@IHostService private readonly hostService: IHostService,
		@IIdentityService private readonly identityService: IIdentityService,
		@IEditorService private readonly editorService: IEditorService
	) {
		super(options, keybindingService, contextMenuService, configurationService, contextKeyService, viewDescriptorService, instantiationService, openerService, themeService, hoverService);

		// Listen for editor changes to track active sessions
		this._register(this.editorService.onDidActiveEditorChange(() => {
			this._updateActiveSessions();
		}));

		this._register(this.editorService.onDidCloseEditor(() => {
			this._updateActiveSessions();
		}));
	}

	protected override renderBody(container: HTMLElement): void {
		super.renderBody(container);

		this._container = container;
		this._container.classList.add('labonair-host-view');

		// Create list container
		this._listContainer = $('div.list-container');
		this._container.appendChild(this._listContainer);

		// Create form container
		this._formContainer = $('div.host-form-overlay.hidden');
		this._container.appendChild(this._formContainer);

		this._renderList();
		this._renderForm();
	}

	private _renderList(): void {
		if (!this._listContainer) {
			return;
		}

		const html = `
			<div class="host-manager">
				<div class="header">
					<div class="quick-connect">
						<input type="text" class="quick-connect-input" placeholder="user@host" />
						<button class="quick-connect-button">Connect</button>
					</div>
					<div class="toolbar">
						<input type="text" class="search-input" placeholder="Search hosts..." />
						<select class="sort-select" id="sortSelect">
							<option value="alphabetical">Sort: Alphabetical</option>
							<option value="lastUsed">Sort: Last Used</option>
							<option value="status">Sort: Status</option>
						</select>
						<button class="add-host-button">Add Host</button>
						<button class="import-button">Import</button>
						<button class="export-button">Export</button>
					</div>
				</div>
				<div class="host-list" id="hostList">
					<div class="loading">Loading hosts...</div>
				</div>
			</div>
		`;

		this._listContainer.innerHTML = html;

		// Add event listeners
		const addButton = this._listContainer.querySelector('.add-host-button');
		if (addButton) {
			this._register(addDisposableListener(addButton, 'click', () => this._showForm(FormMode.Add)));
		}

		const importButton = this._listContainer.querySelector('.import-button');
		if (importButton) {
			this._register(addDisposableListener(importButton, 'click', () => this._handleImport()));
		}

		const exportButton = this._listContainer.querySelector('.export-button');
		if (exportButton) {
			this._register(addDisposableListener(exportButton, 'click', () => this._handleExport()));
		}

		// Sort select
		const sortSelect = this._listContainer.querySelector('#sortSelect') as HTMLSelectElement;
		if (sortSelect) {
			this._register(addDisposableListener(sortSelect, 'change', () => {
				this._sortBy = sortSelect.value as 'alphabetical' | 'lastUsed' | 'status';
				this._loadHosts();
			}));
		}

		this._loadHosts();
	}

	private _renderForm(): void {
		if (!this._formContainer) {
			return;
		}

		this._formContainer.innerHTML = `
			<div class="host-form-header">
				<h2 class="host-form-title" id="formTitle">Add Host</h2>
				<div class="host-form-tabs">
					<div class="host-form-tab active" data-tab="general">General</div>
					<div class="host-form-tab" data-tab="terminal">Terminal</div>
					<div class="host-form-tab" data-tab="filemanager">File Manager</div>
					<div class="host-form-tab" data-tab="advanced">Advanced</div>
				</div>
			</div>
			<div class="host-form-content">
				${this._renderGeneralTab()}
				${this._renderTerminalTab()}
				${this._renderFileManagerTab()}
				${this._renderAdvancedTab()}
			</div>
			<div class="host-form-footer">
				<button class="form-btn form-btn-cancel" id="formCancelBtn">Cancel</button>
				<button class="form-btn form-btn-save" id="formSaveBtn">Save</button>
			</div>
		`;

		this._attachFormEventListeners();
	}

	private _renderGeneralTab(): string {
		return `
			<div class="host-form-section active" data-section="general">
				<!-- Meta Information -->
				<div class="form-group">
					<h3 class="form-group-title">Meta Information</h3>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label required">Name</label>
							<input type="text" class="form-input" id="hostName" placeholder="My Server" />
							<div class="form-error" id="hostNameError"></div>
						</div>

						<div class="form-field">
							<label class="form-label">Group</label>
							<input type="text" class="form-input" id="hostGroup" placeholder="Production" list="groupSuggestions" />
							<datalist id="groupSuggestions"></datalist>
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Tags</label>
							<div class="tag-input-container" id="tagContainer">
								<input type="text" class="tag-input" id="tagInput" placeholder="Add tag..." />
							</div>
						</div>

						<div class="form-field">
							<label class="form-label">OS Icon</label>
							<select class="form-select" id="osIcon">
								<option value="linux">🐧 Linux</option>
								<option value="windows">🪟 Windows</option>
								<option value="macos">🍎 macOS</option>
								<option value="freebsd">👾 FreeBSD</option>
								<option value="unknown">💻 Unknown</option>
							</select>
						</div>
					</div>
				</div>

				<!-- Connection Settings -->
				<div class="form-group">
					<h3 class="form-group-title">Connection</h3>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Protocol</label>
							<div class="segmented-control">
								<button class="segmented-control-item active" data-protocol="ssh">SSH</button>
								<button class="segmented-control-item" data-protocol="local">Local Shell</button>
								<button class="segmented-control-item" data-protocol="wsl">WSL</button>
							</div>
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label required">Host</label>
							<input type="text" class="form-input" id="hostAddress" placeholder="192.168.1.100 or example.com" />
							<div class="form-error" id="hostAddressError"></div>
						</div>

						<div class="form-field" style="max-width: 150px;">
							<label class="form-label required">Port</label>
							<input type="number" class="form-input" id="hostPort" value="22" min="1" max="65535" />
						</div>

						<div class="form-field">
							<label class="form-label required">Username</label>
							<input type="text" class="form-input" id="hostUsername" placeholder="root" />
						</div>
					</div>
				</div>

				<!-- Authentication -->
				<div class="form-group">
					<h3 class="form-group-title">Authentication</h3>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Auth Type</label>
							<div class="segmented-control">
								<button class="segmented-control-item active" data-auth="password">Password</button>
								<button class="segmented-control-item" data-auth="key">Key</button>
								<button class="segmented-control-item" data-auth="agent">Agent</button>
								<button class="segmented-control-item" data-auth="identity_ref">Identity</button>
							</div>
						</div>
					</div>

					<!-- Password Field -->
					<div class="form-row auth-field" id="authPasswordField">
						<div class="form-field">
							<label class="form-label">Password</label>
							<input type="password" class="form-input" id="hostPassword" placeholder="Enter password" />
						</div>
					</div>

					<!-- Key File Field -->
					<div class="form-row auth-field hidden" id="authKeyField">
						<div class="form-field">
							<label class="form-label">Private Key</label>
							<div class="file-upload-area" id="keyUploadArea">
								<div class="file-upload-text">Click to upload or paste key content</div>
								<input type="file" class="file-upload-input" id="keyFileInput" accept=".pem,.key" />
							</div>
							<textarea class="form-textarea" id="keyContent" placeholder="Paste private key here..." style="margin-top: 8px;"></textarea>
						</div>
					</div>

					<!-- Identity Selector -->
					<div class="form-row auth-field hidden" id="authIdentityField">
						<div class="form-field">
							<label class="form-label">Select Identity</label>
							<select class="form-select" id="identitySelector">
								<option value="">Loading identities...</option>
							</select>
						</div>
					</div>

					<!-- Agent Info -->
					<div class="form-row auth-field hidden" id="authAgentField">
						<div class="form-field">
							<div id="agentStatusIndicator" style="padding: 12px; border-radius: 4px; margin-bottom: 8px;">
								<div style="display: flex; align-items: center; gap: 8px;">
									<span class="codicon codicon-loading codicon-modifier-spin"></span>
									<span>Checking SSH agent...</span>
								</div>
							</div>
							<p style="color: var(--vscode-descriptionForeground); font-size: 12px;">
								Will use SSH agent for authentication. Make sure your SSH agent is running and has the key loaded.
							</p>
						</div>
					</div>
				</div>

				<!-- Notes -->
				<div class="form-group">
					<h3 class="form-group-title">Notes</h3>
					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Notes (Markdown supported)</label>
							<textarea class="form-textarea" id="hostNotes" placeholder="Add notes about this host..." rows="4"></textarea>
						</div>
					</div>
				</div>
			</div>
		`;
	}

	private _renderTerminalTab(): string {
		return `
			<div class="host-form-section" data-section="terminal">
				<!-- Appearance -->
				<div class="form-group">
					<h3 class="form-group-title">Appearance</h3>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Cursor Style</label>
							<select class="form-select" id="terminalCursorStyle">
								<option value="block">Block</option>
								<option value="underline">Underline</option>
								<option value="bar">Bar</option>
							</select>
						</div>

						<div class="form-field">
							<label class="form-label">Cursor Blink</label>
							<label class="toggle-switch">
								<input type="checkbox" id="terminalCursorBlink" checked />
								<span class="toggle-slider"></span>
							</label>
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Font Family</label>
							<input type="text" class="form-input" id="terminalFontFamily" placeholder="Menlo, Monaco, Courier New" value="monospace" />
						</div>

						<div class="form-field" style="max-width: 150px;">
							<label class="form-label">Font Size</label>
							<input type="number" class="form-input" id="terminalFontSize" value="14" min="8" max="36" />
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Tab Color</label>
							<input type="color" class="form-color-input" id="terminalTabColor" value="#007acc" />
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px;">
								Sets the border color of the terminal tab
							</small>
						</div>
					</div>
				</div>

				<!-- Behavior -->
				<div class="form-group">
					<h3 class="form-group-title">Behavior</h3>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Copy on Select</label>
							<label class="toggle-switch">
								<input type="checkbox" id="terminalCopyOnSelect" />
								<span class="toggle-slider"></span>
							</label>
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px; display: block; margin-top: 4px;">
								Automatically copy selected text to clipboard
							</small>
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Right Click Behavior</label>
							<select class="form-select" id="terminalRightClickBehavior">
								<option value="menu">Show Context Menu</option>
								<option value="paste">Paste from Clipboard</option>
							</select>
						</div>
					</div>
				</div>
			</div>
		`;
	}

	private _renderFileManagerTab(): string {
		return `
			<div class="host-form-section" data-section="filemanager">
				<!-- Layout -->
				<div class="form-group">
					<h3 class="form-group-title">Layout</h3>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Design Mode</label>
							<div class="segmented-control">
								<button class="segmented-control-item active" data-design="explorer">Explorer (Single Pane)</button>
								<button class="segmented-control-item" data-design="commander">Commander (Dual Pane)</button>
							</div>
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px; display: block; margin-top: 4px;">
								Explorer: Single remote file browser. Commander: Local and remote side-by-side.
							</small>
						</div>
					</div>
				</div>

				<!-- Paths -->
				<div class="form-group">
					<h3 class="form-group-title">Default Paths</h3>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Local Path</label>
							<div style="display: flex; gap: 8px;">
								<input type="text" class="form-input" id="sftpLocalPath" placeholder="/Users/you/Documents" style="flex: 1;" />
								<button class="form-btn-secondary" id="sftpLocalPathBrowse" style="padding: 6px 12px;">Browse</button>
							</div>
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px; display: block; margin-top: 4px;">
								Default local directory for file transfers
							</small>
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Remote Path</label>
							<input type="text" class="form-input" id="sftpRemotePath" placeholder="/home/user" />
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px; display: block; margin-top: 4px;">
								Initial directory to open on the remote server
							</small>
						</div>
					</div>
				</div>

				<!-- Encoding & Options -->
				<div class="form-group">
					<h3 class="form-group-title">File Handling</h3>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">File Encoding</label>
							<select class="form-select" id="sftpEncoding">
								<option value="auto">Auto-detect</option>
								<option value="utf8">UTF-8</option>
								<option value="iso88591">ISO-8859-1 (Latin-1)</option>
								<option value="windows1252">Windows-1252</option>
								<option value="ascii">ASCII</option>
							</select>
						</div>

						<div class="form-field">
							<label class="form-label">Resolve Symlinks</label>
							<label class="toggle-switch">
								<input type="checkbox" id="sftpResolveSymlinks" checked />
								<span class="toggle-slider"></span>
							</label>
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px; display: block; margin-top: 4px;">
								Follow symbolic links to their target
							</small>
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Sudo Save (Elevated Permissions)</label>
							<label class="toggle-switch">
								<input type="checkbox" id="sftpSudoSave" />
								<span class="toggle-slider"></span>
							</label>
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px; display: block; margin-top: 4px;">
								Use sudo when saving files to protected locations
							</small>
						</div>
					</div>
				</div>
			</div>
		`;
	}

	private _renderAdvancedTab(): string {
		return `
			<div class="host-form-section" data-section="advanced">
				<!-- Connection Options -->
				<div class="form-group">
					<h3 class="form-group-title">Connection Options</h3>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Jump Host (Proxy)</label>
							<select class="form-select" id="advancedJumpHost">
								<option value="">None (Direct Connection)</option>
								<!-- Populated dynamically from HostService -->
							</select>
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px; display: block; margin-top: 4px;">
								Connect through another host as a jump server
							</small>
						</div>

						<div class="form-field" style="max-width: 150px;">
							<label class="form-label">Keep-Alive (seconds)</label>
							<input type="number" class="form-input" id="advancedKeepAlive" value="60" min="0" max="600" />
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px; display: block; margin-top: 4px;">
								Send keep-alive packets
							</small>
						</div>
					</div>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Strict Host Key Checking</label>
							<label class="toggle-switch">
								<input type="checkbox" id="advancedStrictHostKey" checked />
								<span class="toggle-slider"></span>
							</label>
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px; display: block; margin-top: 4px;">
								Verify server's host key before connecting
							</small>
						</div>
					</div>
				</div>

				<!-- Port Forwarding / Tunnels -->
				<div class="form-group">
					<h3 class="form-group-title">Port Forwarding</h3>

					<div id="tunnelListContainer">
						<!-- Tunnels will be rendered here dynamically -->
					</div>

					<button class="form-btn-secondary" id="addTunnelBtn" style="margin-top: 8px;">
						<i class="codicon codicon-add"></i> Add Tunnel
					</button>
				</div>

				<!-- Automation -->
				<div class="form-group">
					<h3 class="form-group-title">Automation</h3>

					<div class="form-row">
						<div class="form-field">
							<label class="form-label">Post-Connection Commands</label>
							<textarea class="form-textarea" id="advancedPostExec" placeholder="cd /var/www&#10;export PATH=$PATH:/usr/local/bin" rows="4"></textarea>
							<small style="color: var(--vscode-descriptionForeground); font-size: 11px; display: block; margin-top: 4px;">
								Commands to execute after successful SSH connection (one per line)
							</small>
						</div>
					</div>
				</div>
			</div>
		`;
	}

	private _attachFormEventListeners(): void {
		if (!this._formContainer) {
			return;
		}

		// Tab switching
		const tabs = this._formContainer.querySelectorAll('.host-form-tab');
		tabs.forEach(tab => {
			this._register(addDisposableListener(tab, 'click', () => {
				const tabName = (tab as HTMLElement).dataset['tab'];
				this._switchTab(tabName as string);
			}));
		});

		// Protocol switching
		const protocolButtons = this._formContainer.querySelectorAll('[data-protocol]');
		protocolButtons.forEach(btn => {
			this._register(addDisposableListener(btn, 'click', () => {
				this._handleProtocolChange(btn as HTMLElement);
			}));
		});

		// Auth type switching
		const authButtons = this._formContainer.querySelectorAll('[data-auth]');
		authButtons.forEach(btn => {
			this._register(addDisposableListener(btn, 'click', () => {
				this._handleAuthTypeChange(btn as HTMLElement);
			}));
		});

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
			this._register(addDisposableListener(saveBtn, 'click', () => this._saveHost()));
		}

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

		// Design mode switcher (File Manager tab)
		const designButtons = this._formContainer.querySelectorAll('[data-design]');
		designButtons.forEach(btn => {
			this._register(addDisposableListener(btn, 'click', () => {
				designButtons.forEach(b => b.classList.remove('active'));
				btn.classList.add('active');
			}));
		});

		// Add Tunnel button
		const addTunnelBtn = this._formContainer.querySelector('#addTunnelBtn');
		if (addTunnelBtn) {
			this._register(addDisposableListener(addTunnelBtn, 'click', () => {
				this._addTunnel();
			}));
		}

		// Local path browse button (File Manager tab)
		const sftpLocalPathBrowse = this._formContainer.querySelector('#sftpLocalPathBrowse');
		if (sftpLocalPathBrowse) {
			this._register(addDisposableListener(sftpLocalPathBrowse, 'click', () => {
				// TODO: Open file dialog to select local path
				// This would use IFileDialogService.showOpenDialog
				console.log('Browse local path - to be implemented with file dialog service');
			}));
		}
	}

	private _switchTab(tabName: string): void {
		if (!this._formContainer) {
			return;
		}

		// Update tab buttons
		const tabs = this._formContainer.querySelectorAll('.host-form-tab');
		tabs.forEach(tab => {
			if ((tab as HTMLElement).dataset['tab'] === tabName) {
				tab.classList.add('active');
			} else {
				tab.classList.remove('active');
			}
		});

		// Update sections
		const sections = this._formContainer.querySelectorAll('.host-form-section');
		sections.forEach(section => {
			if ((section as HTMLElement).dataset['section'] === tabName) {
				section.classList.add('active');
			} else {
				section.classList.remove('active');
			}
		});
	}

	private _handleProtocolChange(button: HTMLElement): void {
		if (!this._formContainer) {
			return;
		}

		// Update button states
		const protocolButtons = this._formContainer.querySelectorAll('[data-protocol]');
		protocolButtons.forEach(btn => btn.classList.remove('active'));
		button.classList.add('active');

		// Update form fields based on protocol
		const protocol = button.dataset['protocol'] as HostProtocol;
		const hostAddressField = this._formContainer.querySelector('#hostAddress')?.parentElement?.parentElement;

		if (protocol === 'local' || protocol === 'wsl') {
			hostAddressField?.classList.add('hidden');
		} else {
			hostAddressField?.classList.remove('hidden');
		}
	}

	private _handleAuthTypeChange(button: HTMLElement): void {
		if (!this._formContainer) {
			return;
		}

		// Update button states
		const authButtons = this._formContainer.querySelectorAll('[data-auth]');
		authButtons.forEach(btn => btn.classList.remove('active'));
		button.classList.add('active');

		// Show/hide auth fields
		const authType = button.dataset['auth'] as AuthType;
		const passwordField = this._formContainer.querySelector('#authPasswordField');
		const keyField = this._formContainer.querySelector('#authKeyField');
		const identityField = this._formContainer.querySelector('#authIdentityField');
		const agentField = this._formContainer.querySelector('#authAgentField');

		// Hide all
		[passwordField, keyField, identityField, agentField].forEach(field => {
			field?.classList.add('hidden');
		});

		// Show relevant field
		switch (authType) {
			case 'password':
				passwordField?.classList.remove('hidden');
				break;
			case 'key':
				keyField?.classList.remove('hidden');
				break;
			case 'identity_ref':
				identityField?.classList.remove('hidden');
				this._loadIdentities();
				break;
			case 'agent':
				agentField?.classList.remove('hidden');
				this._checkSSHAgentStatus();
				break;
		}
	}

	/**
	 * Checks SSH agent status and updates the UI indicator
	 * NOTE: Full implementation will be available in Phase 3 when SSH connectivity is added
	 * For now, this shows the UI structure for agent detection
	 */
	private async _checkSSHAgentStatus(): Promise<void> {
		const agentStatusIndicator = this._formContainer?.querySelector('#agentStatusIndicator');
		if (!agentStatusIndicator) {
			return;
		}

		// Show loading state
		agentStatusIndicator.innerHTML = `
			<div style="display: flex; align-items: center; gap: 8px;">
				<span class="codicon codicon-loading codicon-modifier-spin"></span>
				<span>Checking SSH agent...</span>
			</div>
		`;

		try {
			// NOTE: This will be properly implemented in Phase 3
			// For now, we'll check for SSH_AUTH_SOCK environment variable as a basic check
			const hasAuthSock = typeof process !== 'undefined' && process.env && process.env.SSH_AUTH_SOCK;

			setTimeout(() => {
				if (hasAuthSock) {
					agentStatusIndicator.innerHTML = `
						<div style="display: flex; align-items: center; gap: 8px;">
							<span class="codicon codicon-pass-filled" style="color: var(--vscode-testing-iconPassed);"></span>
							<span style="color: var(--vscode-testing-iconPassed);">SSH Agent detected</span>
						</div>
					`;
					(agentStatusIndicator as HTMLElement).style.backgroundColor = 'var(--vscode-inputValidation-infoBackground)';
					(agentStatusIndicator as HTMLElement).style.border = '1px solid var(--vscode-inputValidation-infoBorder)';
				} else {
					agentStatusIndicator.innerHTML = `
						<div style="display: flex; align-items: center; gap: 8px;">
							<span class="codicon codicon-warning" style="color: var(--vscode-inputValidation-warningForeground);"></span>
							<span style="color: var(--vscode-inputValidation-warningForeground);">No SSH Agent found</span>
						</div>
					`;
					(agentStatusIndicator as HTMLElement).style.backgroundColor = 'var(--vscode-inputValidation-warningBackground)';
					(agentStatusIndicator as HTMLElement).style.border = '1px solid var(--vscode-inputValidation-warningBorder)';
				}
			}, 500); // Small delay to show loading state
		} catch (error) {
			agentStatusIndicator.innerHTML = `
				<div style="display: flex; align-items: center; gap: 8px;">
					<span class="codicon codicon-error" style="color: var(--vscode-inputValidation-errorForeground);"></span>
					<span style="color: var(--vscode-inputValidation-errorForeground);">Error checking agent</span>
				</div>
			`;
			(agentStatusIndicator as HTMLElement).style.backgroundColor = 'var(--vscode-inputValidation-errorBackground)';
			(agentStatusIndicator as HTMLElement).style.border = '1px solid var(--vscode-inputValidation-errorBorder)';
		}
	}

	private async _loadIdentities(): Promise<void> {
		const identitySelector = this._formContainer?.querySelector('#identitySelector') as HTMLSelectElement;
		if (!identitySelector) {
			return;
		}

		const identities = await this.identityService.getAllIdentities();

		if (identities.length === 0) {
			identitySelector.innerHTML = '<option value="">No identities configured</option>';
			return;
		}

		const optionsHtml = '<option value="">Select an identity...</option>' +
			identities.map(identity =>
				`<option value="${identity.id}">${identity.name} (${identity.type === 'ssh-key' ? 'SSH Key' : 'Password'})</option>`
			).join('');

		identitySelector.innerHTML = optionsHtml;

		// Select current identity if editing
		if (this._editingHost && this._editingHost.auth.identityId) {
			identitySelector.value = this._editingHost.auth.identityId;
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

	private _addTunnel(): void {
		const newTunnel: IPortTunnel = {
			type: 'local',
			localPort: 8080,
			remoteHost: 'localhost',
			remotePort: 80
		};

		this._tunnels.push(newTunnel);
		this._renderTunnels();
	}

	private _removeTunnel(index: number): void {
		this._tunnels.splice(index, 1);
		this._renderTunnels();
	}

	private _renderTunnels(): void {
		const tunnelListContainer = this._formContainer?.querySelector('#tunnelListContainer');
		if (!tunnelListContainer) {
			return;
		}

		// Clear existing tunnels
		tunnelListContainer.innerHTML = '';

		if (this._tunnels.length === 0) {
			tunnelListContainer.innerHTML = `
				<div style="color: var(--vscode-descriptionForeground); font-size: 12px; padding: 8px; text-align: center;">
					No tunnels configured. Click "Add Tunnel" to create one.
				</div>
			`;
			return;
		}

		// Render each tunnel
		this._tunnels.forEach((tunnel, index) => {
			const tunnelItem = document.createElement('div');
			tunnelItem.className = 'tunnel-item';
			tunnelItem.innerHTML = `
				<div class="tunnel-item-header">
					<select class="form-select tunnel-type" data-index="${index}">
						<option value="local" ${tunnel.type === 'local' ? 'selected' : ''}>Local</option>
						<option value="remote" ${tunnel.type === 'remote' ? 'selected' : ''}>Remote</option>
						<option value="dynamic" ${tunnel.type === 'dynamic' ? 'selected' : ''}>Dynamic</option>
					</select>
					<button class="tunnel-remove-btn" data-index="${index}">
						<i class="codicon codicon-trash"></i>
					</button>
				</div>
				<div class="tunnel-item-fields">
					<div class="form-field">
						<label class="form-label">Local Port</label>
						<input type="number" class="form-input tunnel-local-port" data-index="${index}" value="${tunnel.localPort}" min="1" max="65535" />
					</div>
					<div class="form-field" ${tunnel.type === 'dynamic' ? 'style="display:none;"' : ''}>
						<label class="form-label">Remote Host</label>
						<input type="text" class="form-input tunnel-remote-host" data-index="${index}" value="${tunnel.remoteHost || 'localhost'}" />
					</div>
					<div class="form-field" ${tunnel.type === 'dynamic' ? 'style="display:none;"' : ''}>
						<label class="form-label">Remote Port</label>
						<input type="number" class="form-input tunnel-remote-port" data-index="${index}" value="${tunnel.remotePort || 80}" min="1" max="65535" />
					</div>
				</div>
			`;

			// Add event listeners
			const typeSelect = tunnelItem.querySelector('.tunnel-type') as HTMLSelectElement;
			const removeBtn = tunnelItem.querySelector('.tunnel-remove-btn');
			const localPortInput = tunnelItem.querySelector('.tunnel-local-port') as HTMLInputElement;
			const remoteHostInput = tunnelItem.querySelector('.tunnel-remote-host') as HTMLInputElement;
			const remotePortInput = tunnelItem.querySelector('.tunnel-remote-port') as HTMLInputElement;

			if (typeSelect) {
				this._register(addDisposableListener(typeSelect, 'change', () => {
					this._tunnels[index].type = typeSelect.value as 'local' | 'remote' | 'dynamic';
					this._renderTunnels();
				}));
			}

			if (removeBtn) {
				this._register(addDisposableListener(removeBtn, 'click', () => {
					this._removeTunnel(index);
				}));
			}

			if (localPortInput) {
				this._register(addDisposableListener(localPortInput, 'input', () => {
					this._tunnels[index].localPort = parseInt(localPortInput.value) || 8080;
				}));
			}

			if (remoteHostInput) {
				this._register(addDisposableListener(remoteHostInput, 'input', () => {
					this._tunnels[index].remoteHost = remoteHostInput.value;
				}));
			}

			if (remotePortInput) {
				this._register(addDisposableListener(remotePortInput, 'input', () => {
					this._tunnels[index].remotePort = parseInt(remotePortInput.value) || 80;
				}));
			}

			tunnelListContainer.appendChild(tunnelItem);
		});
	}

	private _showForm(mode: FormMode, host?: IHost): void {
		this._formMode = mode;
		this._editingHost = host || null;
		this._tags = host?.tags || [];
		this._tunnels = host?.tunnels || [];

		if (!this._formContainer) {
			return;
		}

		// Update title
		const title = this._formContainer.querySelector('#formTitle');
		if (title) {
			title.textContent = mode === FormMode.Add ? 'Add Host' : 'Edit Host';
		}

		// Populate form if editing
		if (host) {
			this._populateForm(host);
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
		this._editingHost = null;
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
		const inputs = this._formContainer.querySelectorAll('input, textarea, select');
		inputs.forEach(input => {
			if (input instanceof HTMLInputElement) {
				if (input.type === 'checkbox') {
					input.checked = false;
				} else {
					input.value = '';
				}
			} else if (input instanceof HTMLTextAreaElement || input instanceof HTMLSelectElement) {
				input.value = '';
			}
		});

		// Reset to defaults
		const portInput = this._formContainer.querySelector('#hostPort') as HTMLInputElement;
		if (portInput) {
			portInput.value = '22';
		}

		// Reset tags
		this._tags = [];
		this._renderTags();
	}

	private _populateForm(host: IHost): void {
		if (!this._formContainer) {
			return;
		}

		// Meta
		(this._formContainer.querySelector('#hostName') as HTMLInputElement).value = host.name;
		(this._formContainer.querySelector('#hostGroup') as HTMLInputElement).value = host.group || '';
		(this._formContainer.querySelector('#osIcon') as HTMLSelectElement).value = host.connection.osIcon;

		// Tags
		this._tags = host.tags || [];
		this._renderTags();

		// Connection
		(this._formContainer.querySelector('#hostAddress') as HTMLInputElement).value = host.connection.host;
		(this._formContainer.querySelector('#hostPort') as HTMLInputElement).value = host.connection.port.toString();
		(this._formContainer.querySelector('#hostUsername') as HTMLInputElement).value = host.connection.username;

		// Protocol
		const protocolBtn = this._formContainer.querySelector(`[data-protocol="${host.connection.protocol}"]`);
		if (protocolBtn) {
			this._handleProtocolChange(protocolBtn as HTMLElement);
		}

		// Auth
		const authBtn = this._formContainer.querySelector(`[data-auth="${host.auth.type}"]`);
		if (authBtn) {
			this._handleAuthTypeChange(authBtn as HTMLElement);
		}

		// Terminal
		if (host.terminal) {
			(this._formContainer.querySelector('#terminalCursorStyle') as HTMLSelectElement).value = host.terminal.cursorStyle || 'block';
			(this._formContainer.querySelector('#terminalCursorBlink') as HTMLInputElement).checked = host.terminal.blinking ?? true;
			(this._formContainer.querySelector('#terminalFontFamily') as HTMLInputElement).value = host.terminal.fontFamily || 'monospace';
			(this._formContainer.querySelector('#terminalFontSize') as HTMLInputElement).value = (host.terminal.fontSize || 14).toString();
			(this._formContainer.querySelector('#terminalTabColor') as HTMLInputElement).value = host.terminal.tabColor || '#007acc';
			(this._formContainer.querySelector('#terminalCopyOnSelect') as HTMLInputElement).checked = host.terminal.copyOnSelect || false;
			(this._formContainer.querySelector('#terminalRightClickBehavior') as HTMLSelectElement).value = host.terminal.rightClickBehavior || 'menu';
		}

		// SFTP
		if (host.sftp) {
			const designBtn = this._formContainer.querySelector(`[data-design="${host.sftp.design}"]`);
			if (designBtn) {
				this._formContainer.querySelectorAll('[data-design]').forEach(b => b.classList.remove('active'));
				designBtn.classList.add('active');
			}
			(this._formContainer.querySelector('#sftpLocalPath') as HTMLInputElement).value = host.sftp.localPath || '';
			(this._formContainer.querySelector('#sftpRemotePath') as HTMLInputElement).value = host.sftp.remotePath || '';
			(this._formContainer.querySelector('#sftpResolveSymlinks') as HTMLInputElement).checked = host.sftp.resolveSymlinks ?? true;
			(this._formContainer.querySelector('#sftpSudoSave') as HTMLInputElement).checked = host.sftp.sudoSave || false;
		}

		// Encoding from Advanced
		if (host.advanced) {
			(this._formContainer.querySelector('#sftpEncoding') as HTMLSelectElement).value = host.advanced.encoding || 'auto';
		}

		// Advanced
		if (host.advanced) {
			(this._formContainer.querySelector('#advancedJumpHost') as HTMLSelectElement).value = host.advanced.jumpHostId || '';
			(this._formContainer.querySelector('#advancedKeepAlive') as HTMLInputElement).value = (host.advanced.keepAliveInterval || 60).toString();
			(this._formContainer.querySelector('#advancedStrictHostKey') as HTMLInputElement).checked = true; // Default to true
			(this._formContainer.querySelector('#advancedPostExec') as HTMLTextAreaElement).value = host.advanced.postExecScript?.join('\n') || '';
		}

		// Tunnels
		this._tunnels = host.tunnels || [];
		this._renderTunnels();

		// Notes
		(this._formContainer.querySelector('#hostNotes') as HTMLTextAreaElement).value = host.notes || '';
	}

	private async _saveHost(): Promise<void> {
		if (!this._formContainer) {
			return;
		}

		// Validate
		if (!this._validateForm()) {
			return;
		}

		// Collect form data
		const name = (this._formContainer.querySelector('#hostName') as HTMLInputElement).value;
		const group = (this._formContainer.querySelector('#hostGroup') as HTMLInputElement).value;
		const osIcon = (this._formContainer.querySelector('#osIcon') as HTMLSelectElement).value as OSIcon;
		const hostAddress = (this._formContainer.querySelector('#hostAddress') as HTMLInputElement).value;
		const port = parseInt((this._formContainer.querySelector('#hostPort') as HTMLInputElement).value, 10);
		const username = (this._formContainer.querySelector('#hostUsername') as HTMLInputElement).value;
		const notes = (this._formContainer.querySelector('#hostNotes') as HTMLTextAreaElement).value;

		// Get selected protocol
		const activeProtocolBtn = this._formContainer.querySelector('[data-protocol].active');
		const protocol = (activeProtocolBtn as HTMLElement)?.dataset['protocol'] as HostProtocol || 'ssh';

		// Get selected auth type
		const activeAuthBtn = this._formContainer.querySelector('[data-auth].active');
		const authType = (activeAuthBtn as HTMLElement)?.dataset['auth'] as AuthType || 'password';

		// Terminal settings
		const terminalCursorStyle = (this._formContainer.querySelector('#terminalCursorStyle') as HTMLSelectElement)?.value || 'block';
		const terminalBlinking = (this._formContainer.querySelector('#terminalCursorBlink') as HTMLInputElement)?.checked ?? true;
		const terminalFontFamily = (this._formContainer.querySelector('#terminalFontFamily') as HTMLInputElement)?.value || 'monospace';
		const terminalFontSize = parseInt((this._formContainer.querySelector('#terminalFontSize') as HTMLInputElement)?.value || '14', 10);
		const terminalTabColor = (this._formContainer.querySelector('#terminalTabColor') as HTMLInputElement)?.value || '#007acc';
		const terminalCopyOnSelect = (this._formContainer.querySelector('#terminalCopyOnSelect') as HTMLInputElement)?.checked ?? false;
		const terminalRightClickBehavior = (this._formContainer.querySelector('#terminalRightClickBehavior') as HTMLSelectElement)?.value || 'menu';

		// File Manager settings
		const activeDesignBtn = this._formContainer.querySelector('[data-design].active');
		const sftpDesign = (activeDesignBtn as HTMLElement)?.dataset['design'] || 'explorer';
		const sftpLocalPath = (this._formContainer.querySelector('#sftpLocalPath') as HTMLInputElement)?.value || undefined;
		const sftpRemotePath = (this._formContainer.querySelector('#sftpRemotePath') as HTMLInputElement)?.value || undefined;
		const sftpResolveSymlinks = (this._formContainer.querySelector('#sftpResolveSymlinks') as HTMLInputElement)?.checked ?? true;
		const sftpSudoSave = (this._formContainer.querySelector('#sftpSudoSave') as HTMLInputElement)?.checked ?? false;

		// Advanced settings
		const sftpEncoding = (this._formContainer.querySelector('#sftpEncoding') as HTMLSelectElement)?.value || 'auto';
		const advancedJumpHostId = (this._formContainer.querySelector('#advancedJumpHost') as HTMLSelectElement)?.value || undefined;
		const advancedKeepAliveInterval = parseInt((this._formContainer.querySelector('#advancedKeepAlive') as HTMLInputElement)?.value || '60', 10);
		const advancedPostExecValue = (this._formContainer.querySelector('#advancedPostExec') as HTMLTextAreaElement)?.value || undefined;
		const advancedPostExecScript = advancedPostExecValue ? advancedPostExecValue.split('\n').filter(line => line.trim()) : undefined;

		const host: IHost = {
			id: this._editingHost?.id || generateUuid(),
			name,
			group: group || undefined,
			tags: this._tags,
			connection: {
				host: hostAddress,
				port,
				username,
				protocol,
				osIcon
			},
			auth: {
				type: authType
			},
			terminal: {
				cursorStyle: terminalCursorStyle,
				blinking: terminalBlinking,
				fontFamily: terminalFontFamily,
				fontSize: terminalFontSize,
				tabColor: terminalTabColor,
				copyOnSelect: terminalCopyOnSelect,
				rightClickBehavior: terminalRightClickBehavior as 'menu' | 'paste'
			},
			sftp: {
				design: sftpDesign as 'explorer' | 'commander',
				localPath: sftpLocalPath,
				remotePath: sftpRemotePath,
				resolveSymlinks: sftpResolveSymlinks,
				sudoSave: sftpSudoSave
			},
			advanced: {
				jumpHostId: advancedJumpHostId,
				keepAliveInterval: advancedKeepAliveInterval,
				encoding: sftpEncoding,
				postExecScript: advancedPostExecScript
			},
			tunnels: this._tunnels,
			notes: notes || undefined,
			created: this._editingHost?.created || Date.now()
		};

		try {
			if (this._formMode === FormMode.Edit && this._editingHost) {
				await this.hostService.updateHost(this._editingHost.id, host);
			} else {
				await this.hostService.addHost(host);
			}

			this._hideForm();
			await this._loadHosts();
		} catch (error) {
			console.error('Failed to save host:', error);
			// TODO: Show error message
		}
	}

	private _validateForm(): boolean {
		if (!this._formContainer) {
			return false;
		}

		let isValid = true;

		// Validate name
		const nameInput = this._formContainer.querySelector('#hostName') as HTMLInputElement;
		const nameError = this._formContainer.querySelector('#hostNameError');
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

		// Validate host address
		const hostInput = this._formContainer.querySelector('#hostAddress') as HTMLInputElement;
		const hostError = this._formContainer.querySelector('#hostAddressError');
		if (hostInput.value.trim() && !this._isValidHost(hostInput.value)) {
			hostInput.classList.add('invalid');
			if (hostError) {
				hostError.textContent = 'Invalid hostname or IP address';
			}
			isValid = false;
		} else {
			hostInput.classList.remove('invalid');
			if (hostError) {
				hostError.textContent = '';
			}
		}

		return isValid;
	}

	private _isValidHost(host: string): boolean {
		// Simple hostname/IP validation
		const hostnameRegex = /^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$/;
		const ipRegex = /^(\d{1,3}\.){3}\d{1,3}$/;

		return hostnameRegex.test(host) || ipRegex.test(host);
	}

	/**
	 * Sorts hosts based on the current sort mode
	 */
	private _sortHosts(hosts: IHost[]): IHost[] {
		const hostsCopy = [...hosts];

		switch (this._sortBy) {
			case 'alphabetical':
				return hostsCopy.sort((a, b) => a.name.localeCompare(b.name));

			case 'lastUsed':
				return hostsCopy.sort((a, b) => {
					const aTime = a.lastUsed || 0;
					const bTime = b.lastUsed || 0;
					return bTime - aTime; // Most recently used first
				});

			case 'status':
				return hostsCopy.sort((a, b) => {
					const statusOrder: Record<HostStatus, number> = {
						'online': 0,
						'connecting': 1,
						'unknown': 2,
						'offline': 3
					};

					const aStatus = this.hostService.getHostStatus(a.id);
					const bStatus = this.hostService.getHostStatus(b.id);

					const aOrder = statusOrder[aStatus];
					const bOrder = statusOrder[bStatus];

					if (aOrder !== bOrder) {
						return aOrder - bOrder;
					}

					// If same status, sort alphabetically
					return a.name.localeCompare(b.name);
				});

			default:
				return hostsCopy;
		}
	}

	private async _loadHosts(): Promise<void> {
		const hosts = await this.hostService.getAllHosts();
		const hostListElement = this._listContainer?.querySelector('#hostList');

		if (!hostListElement) {
			return;
		}

		if (hosts.length === 0) {
			hostListElement.innerHTML = '<div class="empty-state">No hosts configured. Click "Add Host" to get started.</div>';
			return;
		}

		// Sort hosts based on current sort mode
		const sortedHosts = this._sortHosts(hosts);

		// Group hosts by group name
		const groupedHosts = new Map<string, IHost[]>();
		const ungroupedHosts: IHost[] = [];

		sortedHosts.forEach(host => {
			if (host.group) {
				if (!groupedHosts.has(host.group)) {
					groupedHosts.set(host.group, []);
				}
				groupedHosts.get(host.group)!.push(host);
			} else {
				ungroupedHosts.push(host);
			}
		});

		let hostsHtml = '';

		// Render grouped hosts
		groupedHosts.forEach((groupHosts, groupName) => {
			hostsHtml += `
				<div class="host-group" data-group-name="${groupName}">
					<div class="host-group-header" data-group-name="${groupName}">
						<span class="group-expand-icon codicon codicon-chevron-down"></span>
						<span class="group-name">${groupName}</span>
						<span class="group-count">(${groupHosts.length})</span>
						<button class="group-settings-btn codicon codicon-settings-gear" data-group-name="${groupName}" title="Group Settings"></button>
					</div>
					<div class="host-group-content">
						${groupHosts.map(host => this._renderHostCard(host)).join('')}
					</div>
				</div>
			`;
		});

		// Render ungrouped hosts
		if (ungroupedHosts.length > 0) {
			hostsHtml += `
				<div class="host-group">
					<div class="host-group-header">
						<span class="group-expand-icon codicon codicon-chevron-down"></span>
						<span class="group-name">Ungrouped</span>
						<span class="group-count">(${ungroupedHosts.length})</span>
					</div>
					<div class="host-group-content">
						${ungroupedHosts.map(host => this._renderHostCard(host)).join('')}
					</div>
				</div>
			`;
		}

		hostListElement.innerHTML = hostsHtml;

		// Add click handlers for group headers (toggle expand/collapse)
		const groupHeaders = hostListElement.querySelectorAll('.host-group-header');
		groupHeaders.forEach(header => {
			this._register(addDisposableListener(header, 'click', (e) => {
				// Don't toggle if clicking the settings button
				if ((e.target as HTMLElement).classList.contains('group-settings-btn')) {
					return;
				}

				const group = header.parentElement;
				const content = group?.querySelector('.host-group-content');
				const icon = header.querySelector('.group-expand-icon');

				if (content && icon) {
					content.classList.toggle('collapsed');
					icon.classList.toggle('codicon-chevron-down');
					icon.classList.toggle('codicon-chevron-right');
				}
			}));
		});

		// Add click handlers for group settings buttons
		const groupSettingsBtns = hostListElement.querySelectorAll('.group-settings-btn');
		groupSettingsBtns.forEach(btn => {
			this._register(addDisposableListener(btn, 'click', (e) => {
				e.stopPropagation();
				const groupName = (btn as HTMLElement).dataset['groupName'];
				if (groupName) {
					this._showGroupSettingsDialog(groupName);
				}
			}));
		});

		// Add click handlers for cards
		const cards = hostListElement.querySelectorAll('.host-card');
		cards.forEach(card => {
			this._register(addDisposableListener(card, 'dblclick', async () => {
				const hostId = (card as HTMLElement).dataset['hostId'];
				if (hostId) {
					const host = await this.hostService.getHost(hostId);
					if (host) {
						this._showForm(FormMode.Edit, host);
					}
				}
			}));
		});
	}

	private _renderHostCard(host: IHost): string {
		const isActive = this._activeSessions.has(host.id);
		const activeClass = isActive ? 'active-session' : '';

		return `
			<div class="host-card ${activeClass}" data-host-id="${host.id}">
				<div class="host-header">
					<span class="os-icon">${this._getOSIcon(host.connection.osIcon)}</span>
					<span class="host-name">${host.name}</span>
					${isActive ? '<span class="active-badge codicon codicon-play"></span>' : ''}
					<span class="status-indicator ${this.hostService.getHostStatus(host.id)}"></span>
				</div>
				<div class="host-info">
					<span class="connection-info">${host.connection.username}@${host.connection.host}:${host.connection.port}</span>
				</div>
				<div class="host-tags">
					${host.tags?.map(tag => `<span class="tag">${tag}</span>`).join('') || ''}
				</div>
			</div>
		`;
	}

	/**
	 * Updates the active sessions by checking all open editors for Labonair remote URIs.
	 * In Phase 3, this will detect actual SSH/SFTP connections.
	 * For now, it's infrastructure for future implementation.
	 */
	private _updateActiveSessions(): void {
		const previousActiveSessions = new Set(this._activeSessions);
		this._activeSessions.clear();

		// Get all editors
		const editors = this.editorService.editors;

		// Check each editor for Labonair remote URIs
		// URI format: labonair-remote://<hostId>/path/to/file
		for (const editor of editors) {
			const resource = editor.resource;
			if (resource && resource.scheme === 'labonair-remote') {
				// Extract host ID from URI authority
				const hostId = resource.authority;
				if (hostId) {
					this._activeSessions.add(hostId);
				}
			}
		}

		// If active sessions changed, reload the host list to update visuals
		const hasChanges =
			previousActiveSessions.size !== this._activeSessions.size ||
			[...previousActiveSessions].some(id => !this._activeSessions.has(id));

		if (hasChanges) {
			this._loadHosts();
		}
	}

	private async _showGroupSettingsDialog(groupName: string): Promise<void> {
		const quickInputService = this.instantiationService.invokeFunction(accessor => accessor.get(IQuickInputService));

		// Get existing group settings
		const groups = await this.hostService.getGroups();
		const group = groups.find(g => g.name === groupName);

		// Get identities for dropdown
		const identities = await this.identityService.getAllIdentities();

		// Create a multi-step quick input
		const disposables: any[] = [];

		try {
			// Step 1: Default Username
			const defaultUsername = await quickInputService.input({
				prompt: `Default username for "${groupName}" group`,
				value: group?.defaults?.connection?.username || '',
				placeHolder: 'e.g., root, admin'
			});

			if (defaultUsername === undefined) {
				return; // User cancelled
			}

			// Step 2: Default Port
			const defaultPort = await quickInputService.input({
				prompt: `Default port for "${groupName}" group`,
				value: group?.defaults?.connection?.port?.toString() || '22',
				placeHolder: '22',
				validateInput: async (value) => {
					const port = parseInt(value, 10);
					if (isNaN(port) || port < 1 || port > 65535) {
						return 'Invalid port number (1-65535)';
					}
					return undefined;
				}
			});

			if (defaultPort === undefined) {
				return; // User cancelled
			}

			// Step 3: Default Identity
			const identityItems = [
				{ label: 'None', id: '' },
				...identities.map(identity => ({
					label: identity.name,
					description: identity.type === 'ssh-key' ? 'SSH Key' : 'Password',
					id: identity.id
				}))
			];

			const selectedIdentity = await quickInputService.pick(identityItems, {
				placeHolder: `Select default identity for "${groupName}" group`,
				title: 'Default Identity'
			});

			if (selectedIdentity === undefined) {
				return; // User cancelled
			}

			// Step 4: Group Color (optional)
			const defaultColor = await quickInputService.input({
				prompt: `Group color for "${groupName}" (optional, hex format)`,
				value: (group?.defaults as any)?.groupColor || '',
				placeHolder: '#007acc'
			});

			// Save group settings
			const groupDefaults: any = {
				connection: {
					username: defaultUsername || undefined,
					port: parseInt(defaultPort, 10) || 22
				},
				auth: selectedIdentity.id ? {
					type: 'identity_ref' as AuthType,
					identityId: selectedIdentity.id
				} : undefined,
				groupColor: defaultColor || undefined
			};

			if (group) {
				await this.hostService.updateGroup(groupName, { defaults: groupDefaults });
			} else {
				await this.hostService.addGroup({ name: groupName, defaults: groupDefaults });
			}

			const notificationService = this.instantiationService.invokeFunction(accessor => accessor.get(INotificationService));
			notificationService.info(`Group "${groupName}" settings updated`);

			// Reload hosts to reflect changes
			await this._loadHosts();

		} finally {
			disposables.forEach(d => d.dispose());
		}
	}

	private _getOSIcon(osIcon: string): string {
		const icons: Record<string, string> = {
			'linux': '🐧',
			'windows': '🪟',
			'macos': '🍎',
			'freebsd': '👾',
			'unknown': '💻'
		};
		return icons[osIcon] || icons['unknown'];
	}

	private async _handleImport(): Promise<void> {
		const quickInputService = this.instantiationService.invokeFunction(accessor => accessor.get(IQuickInputService));
		const fileDialogService = this.instantiationService.invokeFunction(accessor => accessor.get(IFileDialogService));
		const notificationService = this.instantiationService.invokeFunction(accessor => accessor.get(INotificationService));

		// Create QuickPick items
		const items: (IQuickPickItem & { id: string; defaultPath?: () => string | undefined })[] = [
			{
				id: 'labonair',
				label: '$(file-code) Labonair File',
				description: 'Import from .labhosts file',
				detail: 'Import hosts from a Labonair export file'
			},
			{
				id: 'filezilla',
				label: '$(folder) FileZilla',
				description: 'Import from FileZilla sitemanager.xml',
				detail: 'Import FTP/SFTP sites from FileZilla',
				defaultPath: FileZillaImporter.getDefaultPath
			},
			{
				id: 'winscp',
				label: '$(desktop-download) WinSCP',
				description: 'Import from WinSCP.ini',
				detail: 'Import sessions from WinSCP configuration',
				defaultPath: WinSCPImporter.getDefaultPath
			},
			{
				id: 'putty',
				label: '$(terminal) PuTTY',
				description: 'Import from Windows Registry',
				detail: 'Import sessions from PuTTY (Windows only)'
			}
		];

		// Show QuickPick
		const selected = await quickInputService.pick(items, {
			placeHolder: 'Select import source',
			title: 'Import Hosts'
		});

		if (!selected) {
			return; // User cancelled
		}

		// Determine default file path
		let defaultUri: URI | undefined;
		if (selected.defaultPath) {
			const defaultPath = selected.defaultPath();
			if (defaultPath) {
				defaultUri = URI.file(defaultPath);
			}
		}

		// Show file picker
		const fileUris = await fileDialogService.showOpenDialog({
			title: `Select ${selected.label} File`,
			defaultUri,
			canSelectFiles: true,
			canSelectFolders: false,
			canSelectMany: false,
			filters: this._getFileFilters(selected.id)
		});

		if (!fileUris || fileUris.length === 0) {
			return; // User cancelled file selection
		}

		const filePath = fileUris[0].fsPath;

		try {
			// Call appropriate importer
			let result;
			switch (selected.id) {
				case 'labonair':
					result = await LabonairImporter.import(filePath);
					break;
				case 'filezilla':
					result = await FileZillaImporter.import(filePath);
					break;
				case 'winscp':
					result = await WinSCPImporter.import(filePath);
					break;
				case 'putty':
					result = await PuTTYImporter.import();
					break;
				default:
					throw new Error('Unknown import source');
			}

			// Add imported hosts
			for (const host of result.hosts) {
				await this.hostService.addHost(host);
			}

			// Show success notification
			const successMessage = `Successfully imported ${result.hosts.length} host(s)`;
			if (result.errors.length > 0) {
				notificationService.warn(`${successMessage}, but encountered ${result.errors.length} error(s). Check console for details.`);
				console.warn('Import errors:', result.errors);
			} else {
				notificationService.info(successMessage);
			}

			// Refresh host list
			await this._loadHosts();

		} catch (error) {
			notificationService.error(`Import failed: ${error instanceof Error ? error.message : String(error)}`);
		}
	}

	private _getFileFilters(importType: string): { name: string; extensions: string[] }[] {
		switch (importType) {
			case 'labonair':
				return [{ name: 'Labonair Files', extensions: ['labhosts', 'json'] }];
			case 'filezilla':
				return [{ name: 'FileZilla Site Manager', extensions: ['xml'] }];
			case 'winscp':
				return [{ name: 'WinSCP Configuration', extensions: ['ini'] }];
			case 'putty':
				return [{ name: 'Registry Files', extensions: ['reg'] }];
			default:
				return [{ name: 'All Files', extensions: ['*'] }];
		}
	}

	private async _handleExport(): Promise<void> {
		const quickInputService = this.instantiationService.invokeFunction(accessor => accessor.get(IQuickInputService));
		const fileDialogService = this.instantiationService.invokeFunction(accessor => accessor.get(IFileDialogService));
		const notificationService = this.instantiationService.invokeFunction(accessor => accessor.get(INotificationService));

		try {
			// Get all hosts
			const hosts = await this.hostService.getAllHosts();

			if (hosts.length === 0) {
				notificationService.warn('No hosts to export');
				return;
			}

			// Ask about including secrets
			const includeSecrets = await quickInputService.pick([
				{ label: 'Export without passwords', description: 'Safer for sharing', id: 'no' },
				{ label: 'Export with passwords (encrypted)', description: 'Requires encryption password', id: 'yes' }
			], {
				placeHolder: 'Do you want to include passwords?',
				title: 'Export Hosts'
			});

			if (!includeSecrets) {
				return; // User cancelled
			}

			let encryptionPassword: string | undefined;
			if (includeSecrets.id === 'yes') {
				// Ask for encryption password
				encryptionPassword = await quickInputService.input({
					prompt: 'Enter encryption password (minimum 8 characters)',
					password: true,
					validateInput: async (value) => {
						if (!value || value.length < 8) {
							return 'Password must be at least 8 characters';
						}
						return undefined;
					}
				});

				if (!encryptionPassword) {
					return; // User cancelled
				}

				// Confirm password
				const confirmPassword = await quickInputService.input({
					prompt: 'Confirm encryption password',
					password: true,
					validateInput: async (value) => {
						if (value !== encryptionPassword) {
							return 'Passwords do not match';
						}
						return undefined;
					}
				});

				if (!confirmPassword) {
					return; // User cancelled
				}
			}

			// Generate export data
			const exportData: any = {
				version: '1.0',
				exported: new Date().toISOString(),
				hosts: hosts.map(host => {
					const exportHost = { ...host };
					// Remove passwords if not including secrets
					if (includeSecrets.id === 'no' && exportHost.auth) {
						delete (exportHost.auth as any).password;
						delete (exportHost.auth as any).key;
					}
					return exportHost;
				})
			};

			let exportContent = JSON.stringify(exportData, null, 2);

			// Encrypt if password provided
			if (encryptionPassword) {
				// TODO: Implement AES encryption
				// For now, just add a note that encryption is not yet implemented
				notificationService.warn('Note: Encryption not yet implemented. Exporting as plain JSON.');
			}

			// Show save dialog
			const saveUri = await fileDialogService.showSaveDialog({
				title: 'Export Hosts',
				defaultUri: URI.file('labonair-hosts.labhosts'),
				filters: [
					{ name: 'Labonair Hosts', extensions: ['labhosts', 'json'] },
					{ name: 'All Files', extensions: ['*'] }
				]
			});

			if (!saveUri) {
				return; // User cancelled
			}

			// Write file
			const fs = await import('fs');
			fs.writeFileSync(saveUri.fsPath, exportContent, 'utf-8');

			notificationService.info(`Successfully exported ${hosts.length} host(s) to ${saveUri.fsPath}`);

		} catch (error) {
			notificationService.error(`Export failed: ${error instanceof Error ? error.message : String(error)}`);
		}
	}

	protected override layoutBody(height: number, width: number): void {
		super.layoutBody(height, width);

		if (this._container) {
			this._container.style.height = `${height}px`;
			this._container.style.width = `${width}px`;
		}
	}
}
