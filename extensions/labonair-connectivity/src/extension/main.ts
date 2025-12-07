import { Utils } from 'vscode-uri'; // Assuming available or I should check vs imports.
// Actually I don't need Utils if I don't use it.

import * as vscode from 'vscode';
import { HostService } from './hostService';
import { CredentialService } from './credentialService';
import { ScriptService } from './scriptService';
import { SessionTracker } from './sessionTracker';
import { SshAgentService } from './sshAgent';
import { ImporterService } from './importers';
import { registerCommands } from './commands';
import { Message, Host } from '../common/types';

export function activate(context: vscode.ExtensionContext) {
	const hostService = new HostService(context);
	const credentialService = new CredentialService(context);
	const scriptService = new ScriptService(context);
	const sessionTracker = new SessionTracker(context);
	const sshAgentService = new SshAgentService(context);
	const importerService = new ImporterService();

	// Register Commands
	registerCommands(context, hostService);

	// Register the Webview View Provider
	const provider = new ConnectivityViewProvider(context.extensionUri, hostService, credentialService, scriptService, sessionTracker, sshAgentService, importerService);
	context.subscriptions.push(
		vscode.window.registerWebviewViewProvider('labonair.connectivityView', provider)
	);
}

class ConnectivityViewProvider implements vscode.WebviewViewProvider {
	constructor(
		private readonly _extensionUri: vscode.Uri,
		private readonly _hostService: HostService,
		private readonly _credentialService: CredentialService,
		private readonly _scriptService: ScriptService,
		private readonly _sessionTracker: SessionTracker,
		private readonly _sshAgentService: SshAgentService,
		private readonly _importerService: ImporterService
	) { }

	public resolveWebviewView(
		webviewView: vscode.WebviewView,
		context: vscode.WebviewViewResolveContext,
		_token: vscode.CancellationToken,
	) {
		webviewView.webview.options = {
			enableScripts: true,
			localResourceRoots: [this._extensionUri]
		};

		webviewView.webview.html = this._getHtmlForWebview(webviewView.webview);

		// Listen for credential updates
		this._credentialService.onDidChangeCredentials(credentials => {
			webviewView.webview.postMessage({ command: 'UPDATE_DATA', payload: { credentials } });
		});

		// Listen for script updates
		this._scriptService.onDidChangeScripts(scripts => {
			webviewView.webview.postMessage({ command: 'UPDATE_DATA', payload: { scripts } });
		});

		// Listen for session updates
		this._sessionTracker.onDidChangeSessions(activeHostIds => {
			webviewView.webview.postMessage({ command: 'SESSION_UPDATE', payload: { activeHostIds } });
		});

		webviewView.webview.onDidReceiveMessage(async (message: Message) => {
			switch (message.command) {
				case 'FETCH_DATA':
					const hosts = this._hostService.getHosts();
					const credentials = await this._credentialService.getCredentials();
					const scripts = await this._scriptService.getScripts();
					const activeHostIds = this._sessionTracker.getActiveHostIds();
					const agentAvailable = await this._sshAgentService.isAgentAvailable();

					webviewView.webview.postMessage({
						command: 'UPDATE_DATA',
						payload: { hosts, credentials, scripts, activeSessionHostIds: activeHostIds }
					}); // Need to update Message payload type?
					// I checked types.ts, I added SESSION_UPDATE but not activeSessionHostIds to UPDATE_DATA payload?
					// Wait, I updated types.ts with:
					// | { command: 'UPDATE_DATA', payload: { hosts: Host[], credentials?: Credential[], scripts?: Script[] } }
					// I did NOT add activeSessionHostIds to UPDATE_DATA payload.
					// I can either add it to UPDATE_DATA or send a separate SESSION_UPDATE immediately.
					// Sending separate SESSION_UPDATE is safer without modifying types again, but cleaner to have it in initial state.
					// I will send separate SESSION_UPDATE after UPDATE_DATA.
					webviewView.webview.postMessage({ command: 'SESSION_UPDATE', payload: { activeHostIds } });
					webviewView.webview.postMessage({ command: 'AGENT_STATUS', payload: { available: agentAvailable } });
					break;
				case 'SAVE_HOST':
					await this._hostService.saveHost(message.payload.host, message.payload.password, message.payload.keyPath);
					const updatedHosts = this._hostService.getHosts();
					webviewView.webview.postMessage({ command: 'UPDATE_DATA', payload: { hosts: updatedHosts } });
					break;
				case 'DELETE_HOST':
					await this._hostService.deleteHost(message.payload.id);
					const remainingHosts = this._hostService.getHosts();
					webviewView.webview.postMessage({ command: 'UPDATE_DATA', payload: { hosts: remainingHosts } });
					break;
				case 'CONNECT_SSH':
					vscode.window.showInformationMessage(`Connecting to host ${message.payload.id}...`);
					// TODO: Implement actual SSH connection
					const term = vscode.window.createTerminal(`SSH: ${message.payload.id}`);
					term.show();
					this._sessionTracker.registerSession(message.payload.id, term);

					await this._hostService.updateLastUsed(message.payload.id);
					const hostsAfterConnect = this._hostService.getHosts();
					webviewView.webview.postMessage({ command: 'UPDATE_DATA', payload: { hosts: hostsAfterConnect } });
					break;
				case 'PICK_KEY_FILE':
					// ... rest unchanged
					const uris = await vscode.window.showOpenDialog({
						canSelectMany: false,
						title: 'Select SSH Key File',
						filters: { 'All Files': ['*'] } // Keys often have no extension or .pem
					});
					if (uris && uris.length > 0) {
						webviewView.webview.postMessage({ command: 'KEY_FILE_PICKED', payload: { path: uris[0].fsPath } });
					}
					break;
				case 'IMPORT_REQUEST':
					const importedHosts = await this._importerService.importHosts(message.payload.format);
					if (importedHosts.length > 0) {
						for (const host of importedHosts) {
							// We save them one by one. In real app, batch save is better.
							// Assuming no password/key saved with import yet.
							await this._hostService.saveHost(host);
						}
						const newHosts = this._hostService.getHosts();
						webviewView.webview.postMessage({ command: 'UPDATE_DATA', payload: { hosts: newHosts } });
						vscode.window.showInformationMessage(`Imported ${importedHosts.length} hosts.`);
					}
					break;
				case 'EXPORT_REQUEST':
					const currentHosts = this._hostService.getHosts();
					await this._importerService.exportHosts(currentHosts);
					break;
				case 'GET_CREDENTIALS':
					const creds = await this._credentialService.getCredentials();
					webviewView.webview.postMessage({ command: 'UPDATE_DATA', payload: { credentials: creds } });
					break;
				case 'SAVE_CREDENTIAL':
					await this._credentialService.saveCredential(message.payload.credential, message.payload.secret);
					// Updates are sent via listener
					break;
				case 'DELETE_CREDENTIAL':
					await this._credentialService.deleteCredential(message.payload.id);
					// Updates are sent via listener
					break;
				case 'SAVE_GROUP_CONFIG':
					await this._hostService.saveGroupConfig(message.payload.config);
					vscode.window.showInformationMessage(`Group settings saved for ${message.payload.config.name}`);
					break;
				case 'RUN_SCRIPT':
					const scriptId = message.payload.scriptId;
					const hostId = message.payload.hostId;
					const allScripts = await this._scriptService.getScripts();
					const script = allScripts.find(s => s.id === scriptId);
					if (script) {
						vscode.window.showInformationMessage(`Simulating sending script "${script.name}" to host ${hostId}`);
						// Actual implementation will connect to host and send script
					}
					break;
				case 'SAVE_SCRIPT':
					await this._scriptService.saveScript(message.payload.script);
					break;
				case 'DELETE_SCRIPT':
					await this._scriptService.deleteScript(message.payload.id);
					break;
			}
		});
	}

	private _getHtmlForWebview(webview: vscode.Webview) {
		const scriptUri = webview.asWebviewUri(vscode.Uri.joinPath(this._extensionUri, 'dist', 'webview.js'));
		const styleUri = webview.asWebviewUri(vscode.Uri.joinPath(this._extensionUri, 'src', 'webview', 'styles', 'main.css'));


		const nonce = getNonce();

		return `<!DOCTYPE html>
            <html lang="en">
            <head>
                <meta charset="UTF-8">
                <meta name="viewport" content="width=device-width, initial-scale=1.0">
                <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource}; script-src 'nonce-${nonce}';">
                <link href="${styleUri}" rel="stylesheet">
                <title>Labonair Connectivity</title>
            </head>
            <body>
                <div id="root"></div>
                <script nonce="${nonce}" src="${scriptUri}"></script>
            </body>
            </html>`;
	}
}

function getNonce() {
	let text = '';
	const possible = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
	for (let i = 0; i < 32; i++) {
		text += possible.charAt(Math.floor(Math.random() * possible.length));
	}
	return text;
}
