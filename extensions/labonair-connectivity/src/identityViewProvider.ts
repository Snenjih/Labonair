/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import * as vscode from 'vscode';
import { IdentityService, IIdentity } from './identityService';

export class IdentityViewProvider implements vscode.WebviewViewProvider {
	private _view?: vscode.WebviewView;

	constructor(
		private readonly context: vscode.ExtensionContext,
		private readonly identityService: IdentityService
	) {
		this.identityService.onDidChangeIdentities(() => this.refresh());
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

		webviewView.webview.onDidReceiveMessage(async (message) => {
			switch (message.command) {
				case 'refresh':
					await this.refresh();
					break;
				case 'addIdentity':
					await this._handleAddIdentity(message.data);
					break;
				case 'deleteIdentity':
					await this._handleDeleteIdentity(message.data);
					break;
			}
		});

		this.refresh();
	}

	public async refresh() {
		if (this._view) {
			const identities = await this.identityService.getAllIdentities();
			this._view.webview.postMessage({
				command: 'updateIdentities',
				identities
			});
		}
	}

	private async _handleAddIdentity(data: IIdentity & { privateData: string; passphrase?: string }) {
		try {
			await this.identityService.addIdentity(data, data.privateData, data.passphrase);
			vscode.window.showInformationMessage(`Identity "${data.name}" added successfully`);
		} catch (error) {
			vscode.window.showErrorMessage(`Failed to add identity: ${error}`);
		}
	}

	private async _handleDeleteIdentity(data: { id: string }) {
		try {
			const identity = await this.identityService.getIdentity(data.id);
			if (identity) {
				const result = await vscode.window.showWarningMessage(
					`Are you sure you want to delete "${identity.name}"?`,
					{ modal: true },
					'Delete'
				);
				if (result === 'Delete') {
					await this.identityService.deleteIdentity(data.id);
					vscode.window.showInformationMessage('Identity deleted successfully');
				}
			}
		} catch (error) {
			vscode.window.showErrorMessage(`Failed to delete identity: ${error}`);
		}
	}

	private _getHtmlForWebview(_webview: vscode.Webview): string {
		return `<!DOCTYPE html>
		<html lang="en">
		<head>
			<meta charset="UTF-8">
			<meta name="viewport" content="width=device-width, initial-scale=1.0">
			<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-${this._getNonce()}' 'unsafe-inline';">
			<title>Labonair Identities</title>
			<style>
				body { padding: 10px; font-family: var(--vscode-font-family); color: var(--vscode-foreground); }
				.header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px; }
				button { background: var(--vscode-button-background); color: var(--vscode-button-foreground); border: none; padding: 5px 10px; cursor: pointer; }
				.identity-card { border: 1px solid var(--vscode-panel-border); padding: 10px; margin-bottom: 10px; border-radius: 4px; }
				.identity-name { font-weight: bold; }
			</style>
		</head>
		<body>
			<div class="header">
				<h2>Identities</h2>
				<button onclick="addIdentity()">+ Add</button>
			</div>
			<div id="identityList">
				<p>Loading identities...</p>
			</div>
			<script nonce="${this._getNonce()}">
				const vscode = acquireVsCodeApi();

				function refresh() {
					vscode.postMessage({ command: 'refresh' });
				}

				function addIdentity() {
					vscode.postMessage({
						command: 'addIdentity',
						data: {
							id: '',
							name: 'New Identity',
							type: 'ssh-key',
							privateData: '',
							createdAt: Date.now()
						}
					});
				}

				function deleteIdentity(id) {
					vscode.postMessage({ command: 'deleteIdentity', data: { id } });
				}

				window.addEventListener('message', event => {
					const message = event.data;
					if (message.command === 'updateIdentities') {
						renderIdentities(message.identities);
					}
				});

				function renderIdentities(identities) {
					const listEl = document.getElementById('identityList');
					if (!identities || identities.length === 0) {
						listEl.innerHTML = '<p>No identities configured.</p>';
						return;
					}

					listEl.innerHTML = identities.map(identity => \`
						<div class="identity-card">
							<div class="identity-name">\${identity.name}</div>
							<div>Type: \${identity.type}</div>
							<button onclick="deleteIdentity('\${identity.id}')">Delete</button>
						</div>
					\`).join('');
				}

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
