/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import * as vscode from 'vscode';

export type HostProtocol = 'ssh' | 'local' | 'wsl';
export type AuthType = 'password' | 'key' | 'agent' | 'identity_ref';
export type OSIcon = 'linux' | 'windows' | 'macos' | 'freebsd' | 'unknown';
export type SFTPDesign = 'explorer' | 'commander';
export type HostStatus = 'online' | 'offline' | 'unknown' | 'connecting';
export type TunnelType = 'local' | 'remote' | 'dynamic';

export interface IPortTunnel {
	type: TunnelType;
	localPort: number;
	remoteHost: string;
	remotePort: number;
}

export interface IHostAuth {
	type: AuthType;
	identityId?: string;
}

export interface IHostConnection {
	host: string;
	port: number;
	username: string;
	osIcon: OSIcon;
	protocol: HostProtocol;
}

export interface IHostAdvanced {
	jumpHostId?: string;
	proxyCommand?: string;
	keepAliveInterval?: number;
	maxAuthTries?: number;
	encoding?: string;
	postExecScript?: string[];
}

export type RightClickBehavior = 'menu' | 'paste';

export interface IHostTerminal {
	cursorStyle?: string;
	blinking?: boolean;
	tabColor?: string;
	fontFamily?: string;
	fontSize?: number;
	copyOnSelect?: boolean;
	rightClickBehavior?: RightClickBehavior;
}

export interface IHostSFTP {
	design: SFTPDesign;
	layout?: string;
	localPath?: string;
	remotePath?: string;
	sudoSave?: boolean;
	resolveSymlinks?: boolean;
}

export interface IHost {
	id: string;
	name: string;
	group?: string;
	tags?: string[];
	connection: IHostConnection;
	auth: IHostAuth;
	advanced?: IHostAdvanced;
	tunnels?: IPortTunnel[];
	terminal?: IHostTerminal;
	sftp?: IHostSFTP;
	notes?: string;
	status?: HostStatus;
	created?: number;
	lastUsed?: number;
}

export interface IHostGroup {
	name: string;
	defaults?: Partial<IHost>;
}

const HOSTS_STORAGE_KEY = 'labonair.hosts';
const GROUPS_STORAGE_KEY = 'labonair.groups';
const SECRET_KEY_PREFIX = 'labonair.host.secret.';

export class HostService {
	private readonly _onDidChangeHosts = new vscode.EventEmitter<void>();
	readonly onDidChangeHosts: vscode.Event<void> = this._onDidChangeHosts.event;

	private readonly _onDidChangeStatus = new vscode.EventEmitter<{ hostId: string; status: HostStatus }>();
	readonly onDidChangeStatus: vscode.Event<{ hostId: string; status: HostStatus }> = this._onDidChangeStatus.event;

	private _hosts: Map<string, IHost> = new Map();
	private _groups: Map<string, IHostGroup> = new Map();
	private _hostStatus: Map<string, HostStatus> = new Map();
	private _pingInterval: NodeJS.Timeout | undefined;

	constructor(private readonly context: vscode.ExtensionContext) {
		this._loadHosts();
		this._loadGroups();
		this._startBackgroundPing();
	}

	dispose(): void {
		this._stopBackgroundPing();
		this._onDidChangeHosts.dispose();
		this._onDidChangeStatus.dispose();
	}

	private _startBackgroundPing(): void {
		this._pingInterval = setInterval(() => {
			this._pingAllHosts();
		}, 60000); // 60 seconds

		// Initial ping after 5 seconds
		setTimeout(() => {
			this._pingAllHosts();
		}, 5000);
	}

	private _stopBackgroundPing(): void {
		if (this._pingInterval) {
			clearInterval(this._pingInterval);
			this._pingInterval = undefined;
		}
	}

	private async _pingAllHosts(): Promise<void> {
		const hosts = Array.from(this._hosts.values());
		console.log(`[HostService] Background ping starting for ${hosts.length} hosts`);

		for (const host of hosts) {
			// Skip hosts that are being manually refreshed
			if (this._hostStatus.get(host.id) === 'connecting') {
				continue;
			}

			const status = await this._pingHost(host);
			const previousStatus = this._hostStatus.get(host.id);

			if (status !== previousStatus) {
				this._hostStatus.set(host.id, status);
				this._onDidChangeStatus.fire({ hostId: host.id, status });
				console.log(`[HostService] Host ${host.name} status changed: ${previousStatus} -> ${status}`);
			}
		}
	}

	private _loadHosts(): void {
		try {
			const hostsJson = this.context.globalState.get<string>(HOSTS_STORAGE_KEY);
			if (hostsJson) {
				const hosts = JSON.parse(hostsJson) as IHost[];
				hosts.forEach(host => {
					this._hosts.set(host.id, host);
					this._hostStatus.set(host.id, host.status || 'unknown');
				});
				console.log(`[HostService] Loaded ${hosts.length} hosts`);
			}
		} catch (error) {
			console.error('[HostService] Failed to load hosts', error);
		}
	}

	private _loadGroups(): void {
		try {
			const groupsJson = this.context.globalState.get<string>(GROUPS_STORAGE_KEY);
			if (groupsJson) {
				const groups = JSON.parse(groupsJson) as IHostGroup[];
				groups.forEach(group => {
					this._groups.set(group.name, group);
				});
				console.log(`[HostService] Loaded ${groups.length} groups`);
			}
		} catch (error) {
			console.error('[HostService] Failed to load groups', error);
		}
	}

	private async _saveHosts(): Promise<void> {
		try {
			const hosts = Array.from(this._hosts.values());
			const hostsJson = JSON.stringify(hosts);
			await this.context.globalState.update(HOSTS_STORAGE_KEY, hostsJson);
			this._onDidChangeHosts.fire();
			console.log(`[HostService] Saved ${hosts.length} hosts`);
		} catch (error) {
			console.error('[HostService] Failed to save hosts', error);
		}
	}

	private async _saveGroups(): Promise<void> {
		try {
			const groups = Array.from(this._groups.values());
			const groupsJson = JSON.stringify(groups);
			await this.context.globalState.update(GROUPS_STORAGE_KEY, groupsJson);
			console.log(`[HostService] Saved ${groups.length} groups`);
		} catch (error) {
			console.error('[HostService] Failed to save groups', error);
		}
	}

	async getAllHosts(): Promise<IHost[]> {
		return Array.from(this._hosts.values());
	}

	async getHost(id: string): Promise<IHost | undefined> {
		return this._hosts.get(id);
	}

	async addHost(host: IHost): Promise<void> {
		if (!host.id) {
			host.id = this._generateUuid();
		}
		if (!host.created) {
			host.created = Date.now();
		}
		this._hosts.set(host.id, host);
		this._hostStatus.set(host.id, 'unknown');

		if (host.auth.type === 'password') {
			const secretKey = `${SECRET_KEY_PREFIX}${host.id}`;
			await this.context.secrets.store(secretKey, '');
		}

		await this._saveHosts();
		console.log(`[HostService] Added host: ${host.name} (${host.id})`);
	}

	async updateHost(id: string, updates: Partial<IHost>): Promise<void> {
		const host = this._hosts.get(id);
		if (!host) {
			console.warn(`[HostService] Host not found: ${id}`);
			return;
		}

		const updatedHost = { ...host, ...updates };
		this._hosts.set(id, updatedHost);
		await this._saveHosts();
		console.log(`[HostService] Updated host: ${host.name} (${id})`);
	}

	async markHostAsUsed(id: string): Promise<void> {
		const host = this._hosts.get(id);
		if (!host) {
			console.warn(`[HostService] Cannot mark host as used - not found: ${id}`);
			return;
		}

		const updatedHost = { ...host, lastUsed: Date.now() };
		this._hosts.set(id, updatedHost);
		await this._saveHosts();
		console.log(`[HostService] Marked host as used: ${host.name} (${id})`);
	}

	async deleteHost(id: string): Promise<void> {
		const host = this._hosts.get(id);
		if (!host) {
			return;
		}

		this._hosts.delete(id);
		this._hostStatus.delete(id);

		const secretKey = `${SECRET_KEY_PREFIX}${id}`;
		await this.context.secrets.delete(secretKey);

		await this._saveHosts();
		console.log(`[HostService] Deleted host: ${host.name} (${id})`);
	}

	async getGroups(): Promise<IHostGroup[]> {
		return Array.from(this._groups.values());
	}

	async addGroup(group: IHostGroup): Promise<void> {
		this._groups.set(group.name, group);
		await this._saveGroups();
		console.log(`[HostService] Added group: ${group.name}`);
	}

	async updateGroup(name: string, updates: Partial<IHostGroup>): Promise<void> {
		const group = this._groups.get(name);
		if (!group) {
			console.warn(`[HostService] Group not found: ${name}`);
			return;
		}

		const updatedGroup = { ...group, ...updates };
		this._groups.set(name, updatedGroup);
		await this._saveGroups();
		console.log(`[HostService] Updated group: ${name}`);
	}

	async deleteGroup(name: string): Promise<void> {
		this._groups.delete(name);
		await this._saveGroups();
		console.log(`[HostService] Deleted group: ${name}`);
	}

	getHostStatus(id: string): HostStatus {
		return this._hostStatus.get(id) || 'unknown';
	}

	async refreshStatus(id: string): Promise<void> {
		const host = this._hosts.get(id);
		if (!host) {
			return;
		}

		this._hostStatus.set(id, 'connecting');
		this._onDidChangeStatus.fire({ hostId: id, status: 'connecting' });

		const status = await this._pingHost(host);
		this._hostStatus.set(id, status);
		this._onDidChangeStatus.fire({ hostId: id, status });
	}

	private async _pingHost(host: IHost): Promise<HostStatus> {
		// Local and WSL hosts are always available
		if (host.connection.protocol === 'local' || host.connection.protocol === 'wsl') {
			return 'online';
		}

		// For SSH hosts, attempt a basic connectivity check
		try {
			const controller = new AbortController();
			const timeout = setTimeout(() => controller.abort(), 5000);

			// Attempt HTTP/HTTPS check as fallback
			const protocols = ['https://', 'http://'];
			for (const protocol of protocols) {
				try {
					const url = `${protocol}${host.connection.host}`;
					await fetch(url, {
						method: 'HEAD',
						signal: controller.signal,
						mode: 'no-cors'
					});
					clearTimeout(timeout);
					return 'online';
				} catch {
					// Try next protocol
				}
			}

			clearTimeout(timeout);
			return 'unknown';
		} catch (error) {
			return 'unknown';
		}
	}

	async importHosts(source: 'ssh-config' | 'filezilla' | 'labonair', data: string): Promise<IHost[]> {
		console.log(`[HostService] Importing hosts from: ${source}`);

		try {
			let importedHosts: IHost[] = [];

			switch (source) {
				case 'labonair':
					try {
						const parsed = JSON.parse(data) as IHost[];
						importedHosts = parsed.map(host => ({
							...host,
							id: this._generateUuid(),
							created: Date.now()
						}));
					} catch (error) {
						console.error('[HostService] Failed to parse Labonair import data', error);
						throw new Error('Invalid Labonair import format');
					}
					break;

				case 'ssh-config':
				case 'filezilla':
					console.warn(`[HostService] ${source} import not yet implemented`);
					throw new Error(`${source} import not yet implemented`);

				default:
					throw new Error(`Unknown import source: ${source}`);
			}

			for (const host of importedHosts) {
				await this.addHost(host);
			}

			console.log(`[HostService] Successfully imported ${importedHosts.length} hosts from ${source}`);
			return importedHosts;
		} catch (error) {
			console.error(`[HostService] Failed to import hosts from ${source}`, error);
			throw error;
		}
	}

	async exportHosts(hostIds: string[], _encrypt?: boolean): Promise<string> {
		const hosts = hostIds.map(id => this._hosts.get(id)).filter(h => h !== undefined) as IHost[];
		return JSON.stringify(hosts, null, 2);
	}

	getEffectiveConfig(host: IHost): IHost {
		if (!host.group) {
			return host;
		}

		const group = this._groups.get(host.group);
		if (!group || !group.defaults) {
			return host;
		}

		const effective: IHost = JSON.parse(JSON.stringify(host));

		if (group.defaults.connection) {
			effective.connection = {
				...group.defaults.connection,
				...host.connection,
				host: host.connection.host || group.defaults.connection.host,
				port: host.connection.port || group.defaults.connection.port || 22,
				username: host.connection.username || group.defaults.connection.username || 'root',
				protocol: host.connection.protocol || group.defaults.connection.protocol || 'ssh',
				osIcon: host.connection.osIcon || group.defaults.connection.osIcon || 'linux'
			};
		}

		if (group.defaults.auth && !host.auth.identityId && host.auth.type === 'password') {
			effective.auth = {
				...group.defaults.auth,
				...host.auth
			};
		}

		if (group.defaults.advanced) {
			effective.advanced = {
				...group.defaults.advanced,
				...host.advanced,
				jumpHostId: host.advanced?.jumpHostId || group.defaults.advanced.jumpHostId,
				keepAliveInterval: host.advanced?.keepAliveInterval ?? group.defaults.advanced.keepAliveInterval ?? 60,
				encoding: host.advanced?.encoding || group.defaults.advanced.encoding || 'auto'
			};
		}

		if (group.defaults.terminal) {
			effective.terminal = {
				...group.defaults.terminal,
				...host.terminal,
				cursorStyle: host.terminal?.cursorStyle || group.defaults.terminal.cursorStyle || 'block',
				blinking: host.terminal?.blinking ?? group.defaults.terminal.blinking ?? true,
				fontFamily: host.terminal?.fontFamily || group.defaults.terminal.fontFamily || 'monospace',
				fontSize: host.terminal?.fontSize || group.defaults.terminal.fontSize || 14,
				tabColor: host.terminal?.tabColor || group.defaults.terminal.tabColor || '#007acc',
				copyOnSelect: host.terminal?.copyOnSelect ?? group.defaults.terminal.copyOnSelect ?? false,
				rightClickBehavior: host.terminal?.rightClickBehavior || group.defaults.terminal.rightClickBehavior || 'menu'
			};
		}

		if (group.defaults.sftp) {
			effective.sftp = {
				...group.defaults.sftp,
				...host.sftp,
				design: host.sftp?.design || group.defaults.sftp.design || 'explorer',
				resolveSymlinks: host.sftp?.resolveSymlinks ?? group.defaults.sftp.resolveSymlinks ?? true,
				sudoSave: host.sftp?.sudoSave ?? group.defaults.sftp.sudoSave ?? false
			};
		}

		if (group.defaults.tags && host.tags) {
			const combinedTags = [...(group.defaults.tags || []), ...(host.tags || [])];
			effective.tags = Array.from(new Set(combinedTags));
		} else if (group.defaults.tags) {
			effective.tags = group.defaults.tags;
		}

		if (group.defaults.tunnels && host.tunnels) {
			effective.tunnels = [...(group.defaults.tunnels || []), ...(host.tunnels || [])];
		} else if (group.defaults.tunnels) {
			effective.tunnels = group.defaults.tunnels;
		}

		return effective;
	}

	private _generateUuid(): string {
		return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function (c) {
			const r = Math.random() * 16 | 0;
			const v = c === 'x' ? r : (r & 0x3 | 0x8);
			return v.toString(16);
		});
	}
}
