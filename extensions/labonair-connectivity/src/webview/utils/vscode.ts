import type { WebviewMessage } from '../../common/types';

// Declare global vscode acquire function
declare const acquireVsCodeApi: () => {
	postMessage: (message: WebviewMessage) => void;
	getState: () => any;
	setState: (state: any) => void;
};

class VSCodeAPIWrapper {
	private readonly vscodeApi = acquireVsCodeApi();

	public postMessage(message: WebviewMessage) {
		this.vscodeApi.postMessage(message);
	}

	public getState() {
		return this.vscodeApi.getState();
	}

	public setState(state: any) {
		this.vscodeApi.setState(state);
	}
}

// Singleton instance
export const vscodeApi = new VSCodeAPIWrapper();
