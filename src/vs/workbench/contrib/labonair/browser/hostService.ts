/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Disposable } from '../../../../base/common/dispose.js';
import { Emitter, Event } from '../../../../base/common/event.js';
import { IStorageService, StorageScope, StorageTarget } from '../../../../platform/storage/common/storage.js';
import { ISecretStorageService } from '../../../../platform/secrets/common/secrets.js';
import { ILogService } from '../../../../platform/log/common/log.js';
import { IHost, IHostService, HostStatus, IHostGroup } from '../common/hostService.js';
import { HOSTS_DATA_FILE } from '../common/labonair.js';
import { generateUuid } from '../../../../base/common/uuid.js';

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

	constructor(
		@IStorageService private readonly storageService: IStorageService,
		@ISecretStorageService private readonly secretStorageService: ISecretStorageService,
		@ILogService private readonly logService: ILogService
	) {
		super();
		this._loadHosts();
		this._loadGroups();
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
		return Array.from(this._hosts.values());
	}

	async getHost(id: string): Promise<IHost | undefined> {
		return this._hosts.get(id);
	}

	async addHost(host: IHost): Promise<void> {
		if (!host.id) {
			host.id = generateUuid();
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

	private async _pingHost(host: IHost): Promise<HostStatus> {
		if (host.connection.protocol === 'local' || host.connection.protocol === 'wsl') {
			return 'online';
		}

		try {
			const controller = new AbortController();
			const timeout = setTimeout(() => controller.abort(), 5000);

			const url = `http://${host.connection.host}:${host.connection.port}`;
			await fetch(url, {
				method: 'HEAD',
				signal: controller.signal,
				mode: 'no-cors'
			});

			clearTimeout(timeout);
			return 'online';
		} catch (error) {
			return 'offline';
		}
	}

	async importHosts(source: 'ssh-config' | 'filezilla' | 'labonair', data: string): Promise<IHost[]> {
		this.logService.info(`[HostService] Importing hosts from: ${source}`);
		return [];
	}

	async exportHosts(hostIds: string[], encrypt?: boolean): Promise<string> {
		const hosts = hostIds.map(id => this._hosts.get(id)).filter(h => h !== undefined) as IHost[];
		return JSON.stringify(hosts, null, 2);
	}
}
