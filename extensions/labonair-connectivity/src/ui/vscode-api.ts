
// Ein Wrapper für die VS Code Webview API
const vscode = (window as any).acquireVsCodeApi();

// Ersetzt getSSHHosts()
export const getSSHHosts = async () => {
	// Sende Nachricht an Extension
	vscode.postMessage({ command: 'GET_HOSTS' });

	// Warte auf Antwort (Du brauchst hier einen Event Listener Mechanismus)
	return new Promise((resolve) => {
		const handler = (event) => {
			const message = event.data;
			if (message.command === 'SET_HOSTS') {
				window.removeEventListener('message', handler);
				resolve(message.payload);
			}
		};
		window.addEventListener('message', handler);
	});
};

// Ersetzt createSSHHost(data)
export const createSSHHost = async (data: any) => {
	vscode.postMessage({ command: 'SAVE_HOST', data: data });
};

// Ersetzt deleteSSHHost(id)
export const deleteSSHHost = async (id: number) => {
	vscode.postMessage({ command: 'DELETE_HOST', id: id });
};
