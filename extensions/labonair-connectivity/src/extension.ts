import * as vscode from 'vscode';

export class HostManagerProvider implements vscode.WebviewViewProvider {
	constructor(private readonly _extensionUri: vscode.Uri, private readonly _context: vscode.ExtensionContext) { }

	resolveWebviewView(webviewView: vscode.WebviewView) {
		webviewView.webview.options = { enableScripts: true };

		// HTML laden, das deine React App beinhaltet
		webviewView.webview.html = this._getHtmlForWebview(webviewView.webview);

		// Nachrichten vom Frontend (React) empfangen
		webviewView.webview.onDidReceiveMessage(async (data) => {
			switch (data.command) {
				case 'GET_HOSTS':
					const hosts = this._context.globalState.get('hosts', []);
					// Passwörter aus Secrets laden und mergen wäre hier nötig
					webviewView.webview.postMessage({ command: 'SET_HOSTS', payload: hosts });
					break;
				case 'SAVE_HOST':
					const newHost = data.data;
					// Logik zum Speichern in globalState
					// Logik zum Speichern von Passwort in this._context.secrets
					break;
				case 'DELETE_HOST':
					// Lösch-Logik
					break;
			}
		});
	}
	// ... _getHtmlForWebview implementation
}
