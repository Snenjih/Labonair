import * as vscode from 'vscode';
import { HostService } from './hostService';
import { Message } from '../common/types';

export function activate(context: vscode.ExtensionContext) {
	const hostService = new HostService(context);

	// Register the Webview View Provider
	const provider = new ConnectivityViewProvider(context.extensionUri, hostService);
	context.subscriptions.push(
		vscode.window.registerWebviewViewProvider('labonair.connectivityView', provider)
	);
}

class ConnectivityViewProvider implements vscode.WebviewViewProvider {
	constructor(
		private readonly _extensionUri: vscode.Uri,
		private readonly _hostService: HostService
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
			}
		});
	}

	private _getHtmlForWebview(webview: vscode.Webview) {
		const scriptUri = webview.asWebviewUri(vscode.Uri.joinPath(this._extensionUri, 'dist', 'webview.js'));
		const styleUri = webview.asWebviewUri(vscode.Uri.joinPath(this._extensionUri, 'src', 'webview', 'styles', 'main.css'));

		return `<!DOCTYPE html>
            <html lang="en">
            <head>
                <meta charset="UTF-8">
                <meta name="viewport" content="width=device-width, initial-scale=1.0">
                <link href="${styleUri}" rel="stylesheet">
                <title>Labonair Connectivity</title>
            </head>
            <body>
                <div id="root"></div>
                <script src="${scriptUri}"></script>
            </body>
            </html>`;
	}
}
