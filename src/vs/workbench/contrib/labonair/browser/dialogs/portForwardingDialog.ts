/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { IPortTunnel } from '../../common/hostService.js';
import { IDialogService } from '../../../../../platform/dialogs/common/dialogs.js';
import { INotificationService } from '../../../../../platform/notification/common/notification.js';
import { $, addDisposableListener } from '../../../../../base/browser/dom.js';
import { DisposableStore } from '../../../../../base/common/lifecycle.js';

export interface IPortForwardingDialogResult {
	tunnels: IPortTunnel[];
	cancelled: boolean;
}

/**
 * Creates and shows a Port Forwarding Management Dialog
 */
export async function showPortForwardingDialog(
	currentTunnels: IPortTunnel[],
	dialogService: IDialogService,
	notificationService: INotificationService
): Promise<IPortForwardingDialogResult> {
	// Clone tunnels to avoid modifying the original
	const tunnels = currentTunnels.map(t => ({ ...t }));
	const disposables = new DisposableStore();

	return new Promise((resolve) => {
		// Create dialog container
		const container = $('div.port-forwarding-dialog');

		// Render dialog content
		const renderDialog = () => {
			container.innerHTML = '';

			// Header
			const header = $('div.dialog-header');
			header.innerHTML = `
				<h2 style="margin: 0; color: var(--vscode-foreground);">Manage Port Forwarding</h2>
				<p style="margin: 8px 0; color: var(--vscode-descriptionForeground); font-size: 12px;">
					Configure port forwarding tunnels for this host. Tunnels are established when you connect.
				</p>
			`;
			container.appendChild(header);

			// Table
			const tableContainer = $('div.table-container');
			tableContainer.style.cssText = 'max-height: 400px; overflow: auto; margin: 16px 0;';

			if (tunnels.length === 0) {
				const emptyState = $('div.empty-state');
				emptyState.style.cssText = 'padding: 40px; text-align: center; color: var(--vscode-descriptionForeground);';
				emptyState.textContent = 'No port forwarding rules configured. Click "Add Tunnel" to create one.';
				tableContainer.appendChild(emptyState);
			} else {
				const table = $('table.tunnel-table');
				table.style.cssText = 'width: 100%; border-collapse: collapse;';
				table.innerHTML = `
					<thead>
						<tr style="background-color: var(--vscode-list-hoverBackground); border-bottom: 1px solid var(--vscode-panel-border);">
							<th style="padding: 8px; text-align: left; color: var(--vscode-foreground);">Type</th>
							<th style="padding: 8px; text-align: left; color: var(--vscode-foreground);">Local Port</th>
							<th style="padding: 8px; text-align: left; color: var(--vscode-foreground);">Remote Host</th>
							<th style="padding: 8px; text-align: left; color: var(--vscode-foreground);">Remote Port</th>
							<th style="padding: 8px; text-align: center; color: var(--vscode-foreground); width: 60px;">Action</th>
						</tr>
					</thead>
					<tbody>
						${tunnels.map((tunnel, index) => `
							<tr style="border-bottom: 1px solid var(--vscode-panel-border);">
								<td style="padding: 8px;">
									<select class="tunnel-type-select" data-index="${index}" style="
										padding: 4px;
										background-color: var(--vscode-dropdown-background);
										color: var(--vscode-dropdown-foreground);
										border: 1px solid var(--vscode-dropdown-border);
										border-radius: 2px;
									">
										<option value="local" ${tunnel.type === 'local' ? 'selected' : ''}>Local</option>
										<option value="remote" ${tunnel.type === 'remote' ? 'selected' : ''}>Remote</option>
										<option value="dynamic" ${tunnel.type === 'dynamic' ? 'selected' : ''}>Dynamic</option>
									</select>
								</td>
								<td style="padding: 8px;">
									<input type="number" class="tunnel-local-port" data-index="${index}" value="${tunnel.localPort}" min="1" max="65535" style="
										width: 100%;
										padding: 4px;
										background-color: var(--vscode-input-background);
										color: var(--vscode-input-foreground);
										border: 1px solid var(--vscode-input-border);
										border-radius: 2px;
									" />
								</td>
								<td style="padding: 8px;">
									<input type="text" class="tunnel-remote-host" data-index="${index}" value="${tunnel.remoteHost || 'localhost'}"
										${tunnel.type === 'dynamic' ? 'disabled' : ''} style="
										width: 100%;
										padding: 4px;
										background-color: var(--vscode-input-background);
										color: var(--vscode-input-foreground);
										border: 1px solid var(--vscode-input-border);
										border-radius: 2px;
									" />
								</td>
								<td style="padding: 8px;">
									<input type="number" class="tunnel-remote-port" data-index="${index}" value="${tunnel.remotePort || 80}" min="1" max="65535"
										${tunnel.type === 'dynamic' ? 'disabled' : ''} style="
										width: 100%;
										padding: 4px;
										background-color: var(--vscode-input-background);
										color: var(--vscode-input-foreground);
										border: 1px solid var(--vscode-input-border);
										border-radius: 2px;
									" />
								</td>
								<td style="padding: 8px; text-align: center;">
									<button class="tunnel-delete-btn" data-index="${index}" style="
										padding: 4px 8px;
										background-color: var(--vscode-button-secondaryBackground);
										color: var(--vscode-button-secondaryForeground);
										border: none;
										border-radius: 2px;
										cursor: pointer;
									">
										<span class="codicon codicon-trash"></span>
									</button>
								</td>
							</tr>
						`).join('')}
					</tbody>
				`;
				tableContainer.appendChild(table);

				// Add event listeners for table inputs
				const typeSelects = table.querySelectorAll('.tunnel-type-select');
				typeSelects.forEach(select => {
					disposables.add(addDisposableListener(select, 'change', () => {
						const index = parseInt((select as HTMLElement).dataset['index']!, 10);
						tunnels[index].type = (select as HTMLSelectElement).value as 'local' | 'remote' | 'dynamic';
						renderDialog();
					}));
				});

				const localPortInputs = table.querySelectorAll('.tunnel-local-port');
				localPortInputs.forEach(input => {
					disposables.add(addDisposableListener(input, 'input', () => {
						const index = parseInt((input as HTMLElement).dataset['index']!, 10);
						tunnels[index].localPort = parseInt((input as HTMLInputElement).value, 10) || 8080;
					}));
				});

				const remoteHostInputs = table.querySelectorAll('.tunnel-remote-host');
				remoteHostInputs.forEach(input => {
					disposables.add(addDisposableListener(input, 'input', () => {
						const index = parseInt((input as HTMLElement).dataset['index']!, 10);
						tunnels[index].remoteHost = (input as HTMLInputElement).value;
					}));
				});

				const remotePortInputs = table.querySelectorAll('.tunnel-remote-port');
				remotePortInputs.forEach(input => {
					disposables.add(addDisposableListener(input, 'input', () => {
						const index = parseInt((input as HTMLElement).dataset['index']!, 10);
						tunnels[index].remotePort = parseInt((input as HTMLInputElement).value, 10) || 80;
					}));
				});

				const deleteBtns = table.querySelectorAll('.tunnel-delete-btn');
				deleteBtns.forEach(btn => {
					disposables.add(addDisposableListener(btn, 'click', () => {
						const index = parseInt((btn as HTMLElement).dataset['index']!, 10);
						tunnels.splice(index, 1);
						renderDialog();
					}));
				});
			}

			container.appendChild(tableContainer);

			// Add Tunnel button
			const addButton = $('button.add-tunnel-btn');
			addButton.style.cssText = `
				padding: 6px 12px;
				background-color: var(--vscode-button-background);
				color: var(--vscode-button-foreground);
				border: none;
				border-radius: 2px;
				cursor: pointer;
				margin-bottom: 16px;
			`;
			addButton.innerHTML = '<span class="codicon codicon-add"></span> Add Tunnel';
			disposables.add(addDisposableListener(addButton, 'click', () => {
				tunnels.push({
					type: 'local',
					localPort: 8080,
					remoteHost: 'localhost',
					remotePort: 80
				});
				renderDialog();
			}));
			container.appendChild(addButton);

			// Footer buttons
			const footer = $('div.dialog-footer');
			footer.style.cssText = 'display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px;';
			footer.innerHTML = `
				<button class="cancel-btn" style="
					padding: 6px 12px;
					background-color: var(--vscode-button-secondaryBackground);
					color: var(--vscode-button-secondaryForeground);
					border: none;
					border-radius: 2px;
					cursor: pointer;
				">Cancel</button>
				<button class="save-btn" style="
					padding: 6px 12px;
					background-color: var(--vscode-button-background);
					color: var(--vscode-button-foreground);
					border: none;
					border-radius: 2px;
					cursor: pointer;
				">Save</button>
			`;
			container.appendChild(footer);

			const cancelBtn = footer.querySelector('.cancel-btn');
			const saveBtn = footer.querySelector('.save-btn');

			if (cancelBtn) {
				disposables.add(addDisposableListener(cancelBtn, 'click', () => {
					disposables.dispose();
					resolve({ tunnels: currentTunnels, cancelled: true });
				}));
			}

			if (saveBtn) {
				disposables.add(addDisposableListener(saveBtn, 'click', () => {
					// Validate tunnels
					const isValid = validateTunnels(tunnels, notificationService);
					if (isValid) {
						disposables.dispose();
						resolve({ tunnels, cancelled: false });
					}
				}));
			}
		};

		renderDialog();

		// Show dialog using VS Code dialog service
		// Note: In a real implementation, this would use a proper modal dialog
		// For now, we'll simulate it by showing the container in a modal-like way
		const overlay = $('div.port-forwarding-overlay');
		overlay.style.cssText = `
			position: fixed;
			top: 0;
			left: 0;
			right: 0;
			bottom: 0;
			background-color: rgba(0, 0, 0, 0.5);
			display: flex;
			align-items: center;
			justify-content: center;
			z-index: 10000;
		`;

		container.style.cssText = `
			background-color: var(--vscode-editor-background);
			border: 1px solid var(--vscode-panel-border);
			border-radius: 4px;
			padding: 20px;
			min-width: 600px;
			max-width: 800px;
			box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
		`;

		overlay.appendChild(container);
		document.body.appendChild(overlay);

		// Clean up on close
		const originalResolve = resolve;
		resolve = (result) => {
			overlay.remove();
			disposables.dispose();
			originalResolve(result);
		};
	});
}

/**
 * Validates port forwarding tunnels
 */
function validateTunnels(tunnels: IPortTunnel[], notificationService: INotificationService): boolean {
	for (const tunnel of tunnels) {
		// Validate local port
		if (!tunnel.localPort || tunnel.localPort < 1 || tunnel.localPort > 65535) {
			notificationService.error('Invalid local port. Ports must be between 1 and 65535.');
			return false;
		}

		// Validate remote port (if not dynamic)
		if (tunnel.type !== 'dynamic') {
			if (!tunnel.remotePort || tunnel.remotePort < 1 || tunnel.remotePort > 65535) {
				notificationService.error('Invalid remote port. Ports must be between 1 and 65535.');
				return false;
			}

			if (!tunnel.remoteHost || !tunnel.remoteHost.trim()) {
				notificationService.error('Remote host is required for local and remote tunnels.');
				return false;
			}
		}
	}

	return true;
}
