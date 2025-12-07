/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import * as vscode from 'vscode';
import { ScriptService, IScript } from './scriptService';

export class ScriptViewProvider implements vscode.WebviewViewProvider {
	private _view?: vscode.WebviewView;

	constructor(
		private readonly context: vscode.ExtensionContext,
		private readonly scriptService: ScriptService
	) {
		this.scriptService.onDidChangeScripts(() => this.refresh());
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
				case 'addScript':
					await this._handleAddScript(message.data);
					break;
				case 'deleteScript':
					await this._handleDeleteScript(message.data);
					break;
				case 'executeScript':
					await this._handleExecuteScript(message.data);
					break;
			}
		});

		this.refresh();
	}

	public async refresh() {
		if (this._view) {
			const scripts = await this.scriptService.getAllScripts();
			this._view.webview.postMessage({
				command: 'updateScripts',
				scripts
			});
		}
	}

	private async _handleAddScript(data: IScript) {
		try {
			await this.scriptService.addScript(data);
			vscode.window.showInformationMessage(`Script "${data.name}" added successfully`);
		} catch (error) {
			vscode.window.showErrorMessage(`Failed to add script: ${error}`);
		}
	}

	private async _handleDeleteScript(data: { id: string }) {
		try {
			const script = await this.scriptService.getScript(data.id);
			if (script) {
				const result = await vscode.window.showWarningMessage(
					`Are you sure you want to delete "${script.name}"?`,
					{ modal: true },
					'Delete'
				);
				if (result === 'Delete') {
					await this.scriptService.deleteScript(data.id);
					vscode.window.showInformationMessage('Script deleted successfully');
				}
			}
		} catch (error) {
			vscode.window.showErrorMessage(`Failed to delete script: ${error}`);
		}
	}

	private async _handleExecuteScript(data: { scriptId: string; hostId: string }) {
		try {
			await this.scriptService.executeScript(data.scriptId, data.hostId);
			vscode.window.showInformationMessage('Script execution started');
		} catch (error) {
			vscode.window.showErrorMessage(`Failed to execute script: ${error}`);
		}
	}

	private _getHtmlForWebview(_webview: vscode.Webview): string {
		return `<!DOCTYPE html>
		<html lang="en">
		<head>
			<meta charset="UTF-8">
			<meta name="viewport" content="width=device-width, initial-scale=1.0">
			<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-${this._getNonce()}' 'unsafe-inline';">
			<title>Labonair Scripts</title>
			<style>
				body { padding: 10px; font-family: var(--vscode-font-family); color: var(--vscode-foreground); }
				.header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px; }
				button { background: var(--vscode-button-background); color: var(--vscode-button-foreground); border: none; padding: 5px 10px; cursor: pointer; }
				.script-card { border: 1px solid var(--vscode-panel-border); padding: 10px; margin-bottom: 10px; border-radius: 4px; }
				.script-name { font-weight: bold; }
				.script-description { color: var(--vscode-descriptionForeground); font-size: 0.9em; }
			</style>
		</head>
		<body>
			<div class="header">
				<h2>Scripts & Snippets</h2>
				<button onclick="addScript()">+ Add</button>
			</div>
			<div id="scriptList">
				<p>Loading scripts...</p>
			</div>
			<script nonce="${this._getNonce()}">
				const vscode = acquireVsCodeApi();

				function refresh() {
					vscode.postMessage({ command: 'refresh' });
				}

				function addScript() {
					vscode.postMessage({
						command: 'addScript',
						data: {
							id: '',
							name: 'New Script',
							description: 'Script description',
							content: '#!/bin/bash\\necho "Hello World"',
							createdAt: Date.now()
						}
					});
				}

				function deleteScript(id) {
					vscode.postMessage({ command: 'deleteScript', data: { id } });
				}

				window.addEventListener('message', event => {
					const message = event.data;
					if (message.command === 'updateScripts') {
						renderScripts(message.scripts);
					}
				});

				function renderScripts(scripts) {
					const listEl = document.getElementById('scriptList');
					if (!scripts || scripts.length === 0) {
						listEl.innerHTML = '<p>No scripts configured.</p>';
						return;
					}

					listEl.innerHTML = scripts.map(script => \`
						<div class="script-card">
							<div class="script-name">\${script.name}</div>
							<div class="script-description">\${script.description || ''}</div>
							<button onclick="deleteScript('\${script.id}')">Delete</button>
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
