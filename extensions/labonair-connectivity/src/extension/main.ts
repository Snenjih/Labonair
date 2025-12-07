import * as vscode from 'vscode';
import { HostService } from './hostService';
import { ImporterService } from './importers';
import { registerCommands } from './commands';
import { Message, Host } from '../common/types';

export function activate(context: vscode.ExtensionContext) {
	const hostService = new HostService(context);
	const importerService = new ImporterService();

	// Register Commands
	registerCommands(context, hostService);

	// Register the Webview View Provider
	const provider = new ConnectivityViewProvider(context.extensionUri, hostService, importerService);
	context.subscriptions.push(
		vscode.window.registerWebviewViewProvider('labonair.connectivityView', provider)
	);
}

class ConnectivityViewProvider implements vscode.WebviewViewProvider {
	constructor(
		private readonly _extensionUri: vscode.Uri,
		private readonly _hostService: HostService,
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

		webviewView.webview.onDidReceiveMessage(async (message: Message) => {
			switch (message.command) {
				case 'FETCH_DATA':
					const hosts = this._hostService.getHosts();
					webviewView.webview.postMessage({ command: 'UPDATE_DATA', payload: { hosts } });
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
					break;
				case 'PICK_KEY_FILE':
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
