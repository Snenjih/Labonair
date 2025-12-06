/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Disposable } from '../../../../base/common/lifecycle.js';
import { Emitter, Event } from '../../../../base/common/event.js';
import { IStorageService, StorageScope, StorageTarget } from '../../../../platform/storage/common/storage.js';
import { ISecretStorageService } from '../../../../platform/secrets/common/secrets.js';
import { ILogService } from '../../../../platform/log/common/log.js';
import { IHost, IHostService, HostStatus, IHostGroup } from '../common/hostService.js';
import { generateUuid } from '../../../../base/common/uuid.js';
import { ISSHConfigService } from '../common/sshConfigService.js';

const HOSTS_STORAGE_KEY = 'labonair.hosts';
const GROUPS_STORAGE_KEY = 'labonair.groups';
const SECRET_KEY_PREFIX = 'labonair.host.secret.';

export class HostService extends Disposable implements IHostService {
	declare readonly _serviceBrand: undefined;

	private readonly _onDidChangeHosts = this._register(new Emitter<void>());
	readonly onDidChangeHosts: Event<void> = this._onDidChangeHosts.event;

	private readonly _onDidChangeStatus = this._register(new Emitter<{ hostId: string; status: HostStatus }>());
	readonly onDidChangeStatus: Event<{ hostId: string; status: HostStatus }> = this._onDidChangeStatus.event;

	private _hosts: Map<string, IHost> = new Map();
	private _groups: Map<string, IHostGroup> = new Map();
	private _hostStatus: Map<string, HostStatus> = new Map();
	private _pingInterval: any;

	constructor(
		@IStorageService private readonly storageService: IStorageService,
		@ISecretStorageService private readonly secretStorageService: ISecretStorageService,
		@ILogService private readonly logService: ILogService,
		@ISSHConfigService private readonly sshConfigService: ISSHConfigService
	) {
		super();
		this._loadHosts();
		this._loadGroups();
		this._startBackgroundPing();
		this._initSSHConfigIntegration();
	}

	private async _initSSHConfigIntegration(): Promise<void> {
		// Start watching SSH config for changes
		await this.sshConfigService.startWatching();

		// Listen for SSH config changes and notify listeners
		this._register(this.sshConfigService.onDidChangeConfig(() => {
			this.logService.info('[HostService] SSH config changed, firing onDidChangeHosts');
			this._onDidChangeHosts.fire();
		}));
	}

	override dispose(): void {
		this._stopBackgroundPing();
		super.dispose();
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
		this.logService.debug(`[HostService] Background ping starting for ${hosts.length} hosts`);

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
				this.logService.debug(`[HostService] Host ${host.name} status changed: ${previousStatus} -> ${status}`);
			}
		}
	}

	private _loadHosts(): void {
		try {
			const hostsJson = this.storageService.get(HOSTS_STORAGE_KEY, StorageScope.APPLICATION);
			if (hostsJson) {
				const hosts = JSON.parse(hostsJson) as IHost[];
				hosts.forEach(host => {
					this._hosts.set(host.id, host);
					this._hostStatus.set(host.id, host.status || 'unknown');
				});
				this.logService.info(`[HostService] Loaded ${hosts.length} hosts`);
			}
		} catch (error) {
			this.logService.error('[HostService] Failed to load hosts', error);
		}
	}

	private _loadGroups(): void {
		try {
			const groupsJson = this.storageService.get(GROUPS_STORAGE_KEY, StorageScope.APPLICATION);
			if (groupsJson) {
				const groups = JSON.parse(groupsJson) as IHostGroup[];
				groups.forEach(group => {
					this._groups.set(group.name, group);
				});
				this.logService.info(`[HostService] Loaded ${groups.length} groups`);
			}
		} catch (error) {
			this.logService.error('[HostService] Failed to load groups', error);
		}
	}

	private async _saveHosts(): Promise<void> {
		try {
			const hosts = Array.from(this._hosts.values());
			const hostsJson = JSON.stringify(hosts);
			this.storageService.store(HOSTS_STORAGE_KEY, hostsJson, StorageScope.APPLICATION, StorageTarget.USER);
			this._onDidChangeHosts.fire();
			this.logService.info(`[HostService] Saved ${hosts.length} hosts`);
		} catch (error) {
			this.logService.error('[HostService] Failed to save hosts', error);
		}
	}

	private async _saveGroups(): Promise<void> {
		try {
			const groups = Array.from(this._groups.values());
			const groupsJson = JSON.stringify(groups);
			this.storageService.store(GROUPS_STORAGE_KEY, groupsJson, StorageScope.APPLICATION, StorageTarget.USER);
			this.logService.info(`[HostService] Saved ${groups.length} groups`);
		} catch (error) {
			this.logService.error('[HostService] Failed to save groups', error);
		}
	}

	async getAllHosts(): Promise<IHost[]> {
		const userHosts = Array.from(this._hosts.values());
		const systemHosts = await this.sshConfigService.getSystemHosts();
		return [...userHosts, ...systemHosts];
	}

	async getHost(id: string): Promise<IHost | undefined> {
		// First check user hosts
		const userHost = this._hosts.get(id);
		if (userHost) {
			return userHost;
		}

		// Then check system hosts
		const systemHosts = await this.sshConfigService.getSystemHosts();
		return systemHosts.find(h => h.id === id);
	}

	async addHost(host: IHost): Promise<void> {
		if (!host.id) {
			host.id = generateUuid();
		}
		if (!host.created) {
			host.created = Date.now();
		}
		this._hosts.set(host.id, host);
		this._hostStatus.set(host.id, 'unknown');

		if (host.auth.type === 'password') {
			const secretKey = `${SECRET_KEY_PREFIX}${host.id}`;
			await this.secretStorageService.set(secretKey, '');
		}

		await this._saveHosts();
		this.logService.info(`[HostService] Added host: ${host.name} (${host.id})`);
	}

	async updateHost(id: string, updates: Partial<IHost>): Promise<void> {
		const host = this._hosts.get(id);
		if (!host) {
			this.logService.warn(`[HostService] Host not found: ${id}`);
			return;
		}

		const updatedHost = { ...host, ...updates };
		this._hosts.set(id, updatedHost);
		await this._saveHosts();
		this.logService.info(`[HostService] Updated host: ${host.name} (${id})`);
	}

	async deleteHost(id: string): Promise<void> {
		const host = this._hosts.get(id);
		if (!host) {
			return;
		}

		this._hosts.delete(id);
		this._hostStatus.delete(id);

		const secretKey = `${SECRET_KEY_PREFIX}${id}`;
		await this.secretStorageService.delete(secretKey);

		await this._saveHosts();
		this.logService.info(`[HostService] Deleted host: ${host.name} (${id})`);
	}

	async getGroups(): Promise<IHostGroup[]> {
		return Array.from(this._groups.values());
	}

	async addGroup(group: IHostGroup): Promise<void> {
		this._groups.set(group.name, group);
		await this._saveGroups();
		this.logService.info(`[HostService] Added group: ${group.name}`);
	}

	async updateGroup(name: string, updates: Partial<IHostGroup>): Promise<void> {
		const group = this._groups.get(name);
		if (!group) {
			this.logService.warn(`[HostService] Group not found: ${name}`);
			return;
		}

		const updatedGroup = { ...group, ...updates };
		this._groups.set(name, updatedGroup);
		await this._saveGroups();
		this.logService.info(`[HostService] Updated group: ${name}`);
	}

	async deleteGroup(name: string): Promise<void> {
		this._groups.delete(name);
		await this._saveGroups();
		this.logService.info(`[HostService] Deleted group: ${name}`);
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

	/**
	 * Pings a host to determine its online status.
	 * NOTE: This is a browser-based implementation with limitations.
	 * In Phase 3, this will be replaced with proper TCP socket-based ping
	 * using Node.js net.Socket for accurate SSH port connectivity checks.
	 *
	 * Current behavior:
	 * - Local/WSL hosts are always considered online
	 * - SSH hosts use HTTP fallback (limited accuracy)
	 */
	private async _pingHost(host: IHost): Promise<HostStatus> {
		// Local and WSL hosts are always available
		if (host.connection.protocol === 'local' || host.connection.protocol === 'wsl') {
			return 'online';
		}

		// For SSH hosts, attempt a basic connectivity check
		// This is a placeholder until proper TCP socket ping is implemented in Phase 3
		try {
			const controller = new AbortController();
			const timeout = setTimeout(() => controller.abort(), 5000);

			// Attempt HTTP/HTTPS check as fallback
			// This won't work for SSH-only hosts but is the best we can do in browser context
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
			// If no HTTP/HTTPS response, mark as unknown (not offline)
			// since the host might still be reachable via SSH
			return 'unknown';
		} catch (error) {
			return 'unknown';
		}
	}

	async importHosts(source: 'ssh-config' | 'filezilla' | 'labonair', data: string): Promise<IHost[]> {
		this.logService.info(`[HostService] Importing hosts from: ${source}`);

		try {
			let importedHosts: IHost[] = [];

			switch (source) {
				case 'ssh-config':
					// SSH config hosts are already available through the SSHConfigService
					// Convert them to Labonair hosts
					const systemHosts = await this.sshConfigService.getSystemHosts();
					importedHosts = systemHosts.map(host =>
						this.sshConfigService.convertToLabonairHost(host)
					);
					break;

				case 'labonair':
					// Direct import from Labonair JSON format
					try {
						const parsed = JSON.parse(data) as IHost[];
						importedHosts = parsed.map(host => ({
							...host,
							id: generateUuid(), // Generate new IDs for imported hosts
							created: Date.now()
						}));
					} catch (error) {
						this.logService.error('[HostService] Failed to parse Labonair import data', error);
						throw new Error('Invalid Labonair import format');
					}
					break;

				case 'filezilla':
					// TODO: Implement FileZilla XML parsing in Phase 3
					this.logService.warn('[HostService] FileZilla import not yet implemented');
					throw new Error('FileZilla import not yet implemented');

				default:
					throw new Error(`Unknown import source: ${source}`);
			}

			// Add imported hosts to storage
			for (const host of importedHosts) {
				await this.addHost(host);
			}

			this.logService.info(`[HostService] Successfully imported ${importedHosts.length} hosts from ${source}`);
			return importedHosts;
		} catch (error) {
			this.logService.error(`[HostService] Failed to import hosts from ${source}`, error);
			throw error;
		}
	}

	async exportHosts(hostIds: string[], encrypt?: boolean): Promise<string> {
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

		const effective: IHost = { ...host };

		if (group.defaults.connection) {
			effective.connection = {
				...group.defaults.connection,
				...host.connection
			};
		}

		if (group.defaults.auth && !host.auth.identityId) {
			effective.auth = {
				...group.defaults.auth,
				...host.auth
			};
		}

		if (group.defaults.advanced) {
			effective.advanced = {
				...group.defaults.advanced,
				...host.advanced
			};
		}

		if (group.defaults.terminal) {
			effective.terminal = {
				...group.defaults.terminal,
				...host.terminal
			};
		}

		if (group.defaults.sftp) {
			effective.sftp = {
				...group.defaults.sftp,
				...host.sftp
			};
		}

		return effective;
	}
}
