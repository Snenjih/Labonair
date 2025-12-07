/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import * as vscode from 'vscode';
import { HostService, IHost } from './hostService';

export class HostViewProvider implements vscode.WebviewViewProvider {
	private _view?: vscode.WebviewView;

	constructor(
		private readonly context: vscode.ExtensionContext,
		private readonly hostService: HostService
	) {
		// Listen for host changes
		this.hostService.onDidChangeHosts(() => this.refresh());
		this.hostService.onDidChangeStatus(() => this.refresh());
	}

	public resolveWebviewView(
		webviewView: vscode.WebviewView,
		_context: vscode.WebviewViewResolveContext,
		_token: vscode.CancellationToken
	) {
		this._view = webviewView;

		webviewView.webview.options = {
			enableScripts: true,
			localResourceRoots: [
				vscode.Uri.joinPath(this.context.extensionUri, 'media')
			]
		};

		webviewView.webview.html = this._getHtmlForWebview(webviewView.webview);

		// Handle messages from the webview
		webviewView.webview.onDidReceiveMessage(async (message) => {
			switch (message.command) {
				case 'refresh':
					await this.refresh();
					break;
				case 'addHost':
					await this._handleAddHost(message.data);
					break;
				case 'updateHost':
					await this._handleUpdateHost(message.data);
					break;
				case 'deleteHost':
					await this._handleDeleteHost(message.data);
					break;
				case 'connectHost':
					await this._handleConnectHost(message.data);
					break;
				default:
					console.warn(`[HostViewProvider] Unknown command: ${message.command}`);
			}
		});

		// Initial render
		this.refresh();
	}

	public async refresh() {
		if (this._view) {
			const hosts = await this.hostService.getAllHosts();
			this._view.webview.postMessage({
				command: 'updateHosts',
				hosts: hosts.map(host => ({
					...host,
					status: this.hostService.getHostStatus(host.id)
				}))
			});
		}
	}

	private async _handleAddHost(data: IHost) {
		try {
			await this.hostService.addHost(data);
			vscode.window.showInformationMessage(`Host "${data.name}" added successfully`);
		} catch (error) {
			vscode.window.showErrorMessage(`Failed to add host: ${error}`);
		}
	}

	private async _handleUpdateHost(data: { id: string; updates: Partial<IHost> }) {
		try {
			await this.hostService.updateHost(data.id, data.updates);
			vscode.window.showInformationMessage('Host updated successfully');
		} catch (error) {
			vscode.window.showErrorMessage(`Failed to update host: ${error}`);
		}
	}

	private async _handleDeleteHost(data: { id: string }) {
		try {
			const host = await this.hostService.getHost(data.id);
			if (host) {
				const result = await vscode.window.showWarningMessage(
					`Are you sure you want to delete "${host.name}"?`,
					{ modal: true },
					'Delete'
				);
				if (result === 'Delete') {
					await this.hostService.deleteHost(data.id);
					vscode.window.showInformationMessage('Host deleted successfully');
				}
			}
		} catch (error) {
			vscode.window.showErrorMessage(`Failed to delete host: ${error}`);
		}
	}

	private async _handleConnectHost(data: { id: string }) {
		try {
			const host = await this.hostService.getHost(data.id);
			if (host) {
				await this.hostService.markHostAsUsed(data.id);
				vscode.window.showInformationMessage(`Connecting to ${host.name}...`);
				// NOTE: Actual SSH connection will be implemented in Phase 3
			}
		} catch (error) {
			vscode.window.showErrorMessage(`Failed to connect to host: ${error}`);
		}
	}

	private _getHtmlForWebview(webview: vscode.Webview): string {
		const styleUri = webview.asWebviewUri(
			vscode.Uri.joinPath(this.context.extensionUri, 'media', 'hostManager.css')
		);

		return `<!DOCTYPE html>
		<html lang="en">
		<head>
			<meta charset="UTF-8">
			<meta name="viewport" content="width=device-width, initial-scale=1.0">
			<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource} 'unsafe-inline'; script-src 'nonce-${this._getNonce()}' 'unsafe-inline';">
			<link href="${styleUri}" rel="stylesheet">
			<title>Labonair Hosts</title>
		</head>
		<body>
			<div class="header">
				<h2>Hosts</h2>
				<button onclick="addHost()">+ Add Host</button>
				<button onclick="refresh()">Refresh</button>
			</div>
			<div id="hostList" class="host-list">
				<p>Loading hosts...</p>
			</div>
			<script nonce="${this._getNonce()}">
				const vscode = acquireVsCodeApi();

				function refresh() {
					vscode.postMessage({ command: 'refresh' });
				}

				function addHost() {
					vscode.postMessage({
						command: 'addHost',
						data: {
							id: '',
							name: 'New Host',
							connection: {
								host: '192.168.1.1',
								port: 22,
								username: 'root',
								osIcon: 'linux',
								protocol: 'ssh'
							},
							auth: {
								type: 'password'
							}
						}
					});
				}

				function connectHost(id) {
					vscode.postMessage({ command: 'connectHost', data: { id } });
				}

				function deleteHost(id) {
					vscode.postMessage({ command: 'deleteHost', data: { id } });
				}

				window.addEventListener('message', event => {
					const message = event.data;
					switch (message.command) {
						case 'updateHosts':
							renderHosts(message.hosts);
							break;
					}
				});

				function renderHosts(hosts) {
					const listEl = document.getElementById('hostList');
					if (!hosts || hosts.length === 0) {
						listEl.innerHTML = '<p>No hosts configured. Click "Add Host" to get started.</p>';
						return;
					}

					listEl.innerHTML = hosts.map(host => \`
						<div class="host-card">
							<div class="host-header">
								<span class="host-name">\${host.name}</span>
								<span class="host-status status-\${host.status}">\${host.status || 'unknown'}</span>
							</div>
							<div class="host-info">
								<p>\${host.connection.username}@\${host.connection.host}:\${host.connection.port}</p>
								<p>Protocol: \${host.connection.protocol}</p>
							</div>
							<div class="host-actions">
								<button onclick="connectHost('\${host.id}')">Connect</button>
								<button onclick="deleteHost('\${host.id}')">Delete</button>
							</div>
						</div>
					\`).join('');
				}

				// Request initial data
				refresh();
			</script>
		</body>
		</html>`;
	}

	private _getNonce(): string {
		let text = '';
		const possible = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
		for (let i = 0; i < 32; i++) {
			text += possible.charAt(Math.floor(Math.random() * possible.length));
		}
		return text;
	}
}
