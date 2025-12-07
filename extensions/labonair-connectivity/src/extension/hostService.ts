import * as vscode from 'vscode';
import { Host } from '../common/types';

export class HostService {
	private context: vscode.ExtensionContext;
	private readonly STORAGE_KEY = 'labonair.hosts';

	constructor(context: vscode.ExtensionContext) {
		this.context = context;
	}

	public getHosts(): Host[] {
		return this.context.globalState.get<Host[]>(this.STORAGE_KEY, []);
	}

	public async saveHost(host: Host, password?: string, keyPath?: string): Promise<void> {
		const hosts = this.getHosts();
		const index = hosts.findIndex(h => h.id === host.id);

		if (index !== -1) {
			hosts[index] = host;
		} else {
			hosts.push(host);
		}

		await this.context.globalState.update(this.STORAGE_KEY, hosts);

		if (password) {
			await this.context.secrets.store(`pwd.${host.id}`, password);
		}
	}

	public async deleteHost(id: string): Promise<void> {
		const hosts = this.getHosts().filter(h => h.id !== id);
		await this.context.globalState.update(this.STORAGE_KEY, hosts);
		await this.context.secrets.delete(`pwd.${id}`);
	}
}
