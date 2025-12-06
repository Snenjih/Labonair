/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Disposable } from '../../../../base/common/dispose.js';
import { IViewPaneOptions, ViewPane } from '../../../browser/parts/views/viewPane.js';
import { IKeybindingService } from '../../../../platform/keybinding/common/keybinding.js';
import { IContextMenuService } from '../../../../platform/contextview/browser/contextView.js';
import { IConfigurationService } from '../../../../platform/configuration/common/configuration.js';
import { IContextKeyService } from '../../../../platform/contextkey/common/contextkey.js';
import { IViewDescriptorService } from '../../../common/views.js';
import { IInstantiationService } from '../../../../platform/instantiation/common/instantiation.js';
import { IOpenerService } from '../../../../platform/opener/common/opener.js';
import { IThemeService } from '../../../../platform/theme/common/themeService.js';
import { ITelemetryService } from '../../../../platform/telemetry/common/telemetry.js';
import { IHostService } from '../common/hostService.js';
import { IIdentityService } from '../common/identityService.js';
import { $, addDisposableListener, append, Dimension } from '../../../../base/browser/dom.js';

export class LabonairHostView extends ViewPane {
	private _container?: HTMLElement;
	private _webviewContainer?: HTMLElement;

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
		@ITelemetryService telemetryService: ITelemetryService,
		@IHostService private readonly hostService: IHostService,
		@IIdentityService private readonly identityService: IIdentityService
	) {
		super(options, keybindingService, contextMenuService, configurationService, contextKeyService, viewDescriptorService, instantiationService, openerService, themeService, telemetryService);
	}

	protected override renderBody(container: HTMLElement): void {
		super.renderBody(container);

		this._container = container;
		this._container.classList.add('labonair-host-view');

		this._webviewContainer = append(this._container, $('.webview-container'));
		this._renderWebview();
	}

	private _renderWebview(): void {
		if (!this._webviewContainer) {
			return;
		}

		const html = this._getWebviewContent();
		this._webviewContainer.innerHTML = html;

		this._register(addDisposableListener(this._webviewContainer, 'click', (e: MouseEvent) => {
			const target = e.target as HTMLElement;
			if (target.classList.contains('add-host-button')) {
				this._handleAddHost();
			} else if (target.classList.contains('host-card')) {
				const hostId = target.dataset['hostId'];
				if (hostId) {
					this._handleHostClick(hostId);
				}
			}
		}));

		this._loadHosts();
	}

	private _getWebviewContent(): string {
		return `
			<div class="host-manager">
				<div class="header">
					<div class="quick-connect">
						<input type="text" class="quick-connect-input" placeholder="user@host" />
						<button class="quick-connect-button">Connect</button>
					</div>
					<div class="toolbar">
						<input type="text" class="search-input" placeholder="Search hosts..." />
						<button class="add-host-button">Add Host</button>
					</div>
				</div>
				<div class="host-list" id="hostList">
					<div class="loading">Loading hosts...</div>
				</div>
			</div>
		`;
	}

	private async _loadHosts(): Promise<void> {
		const hosts = await this.hostService.getAllHosts();
		const hostListElement = this._webviewContainer?.querySelector('#hostList');

		if (!hostListElement) {
			return;
		}

		if (hosts.length === 0) {
			hostListElement.innerHTML = '<div class="empty-state">No hosts configured. Click "Add Host" to get started.</div>';
			return;
		}

		const hostsHtml = hosts.map(host => `
			<div class="host-card" data-host-id="${host.id}">
				<div class="host-header">
					<span class="os-icon">${this._getOSIcon(host.connection.osIcon)}</span>
					<span class="host-name">${host.name}</span>
					<span class="status-indicator ${this.hostService.getHostStatus(host.id)}"></span>
				</div>
				<div class="host-info">
					<span class="connection-info">${host.connection.username}@${host.connection.host}:${host.connection.port}</span>
				</div>
				<div class="host-tags">
					${host.tags?.map(tag => `<span class="tag">${tag}</span>`).join('') || ''}
				</div>
			</div>
		`).join('');

		hostListElement.innerHTML = hostsHtml;
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

	private _handleAddHost(): void {
		console.log('Add host clicked');
	}

	private _handleHostClick(hostId: string): void {
		console.log('Host clicked:', hostId);
	}

	protected override layoutBody(height: number, width: number): void {
		super.layoutBody(height, width);

		if (this._container) {
			this._container.style.height = `${height}px`;
			this._container.style.width = `${width}px`;
		}
	}
}
