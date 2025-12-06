/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Disposable } from '../../../../base/common/lifecycle.js';
import { Emitter, Event } from '../../../../base/common/event.js';
import { ILogService } from '../../../../platform/log/common/log.js';
import { IHost } from '../common/hostService.js';
import { ISSHConfigService } from '../common/sshConfigService.js';
import { SSHConfigParser } from './sshConfigParser.js';
import { generateUuid } from '../../../../base/common/uuid.js';

/**
 * SSH Config Service implementation for Node.js environment.
 * Manages parsing and watching of the system SSH config file.
 */
export class SSHConfigService extends Disposable implements ISSHConfigService {
	declare readonly _serviceBrand: undefined;

	private readonly _onDidChangeConfig = this._register(new Emitter<void>());
	readonly onDidChangeConfig: Event<void> = this._onDidChangeConfig.event;

	private _systemHosts: Map<string, IHost> = new Map();
	private _unwatchConfig: (() => void) | undefined;
	private _isWatching: boolean = false;

	constructor(
		@ILogService private readonly logService: ILogService
	) {
		super();
		this._loadSystemHosts();
	}

	override dispose(): void {
		this.stopWatching();
		super.dispose();
	}

	private async _loadSystemHosts(): Promise<void> {
		try {
			const hosts = await SSHConfigParser.parse();
			this._systemHosts.clear();
			hosts.forEach(host => {
				this._systemHosts.set(host.name, host);
			});
			this.logService.info(`[SSHConfigService] Loaded ${hosts.length} hosts from SSH config`);
		} catch (error) {
			this.logService.error('[SSHConfigService] Failed to load SSH config', error);
		}
	}

	async getSystemHosts(): Promise<IHost[]> {
		return Array.from(this._systemHosts.values());
	}

	async getSystemHost(alias: string): Promise<IHost | undefined> {
		return this._systemHosts.get(alias);
	}

	async startWatching(): Promise<void> {
		if (this._isWatching) {
			return;
		}

		try {
			this._unwatchConfig = await SSHConfigParser.watch(async (hosts) => {
				this._systemHosts.clear();
				hosts.forEach(host => {
					this._systemHosts.set(host.name, host);
				});
				this._onDidChangeConfig.fire();
				this.logService.info(`[SSHConfigService] SSH config changed, reloaded ${hosts.length} hosts`);
			});
			this._isWatching = true;
			this.logService.info('[SSHConfigService] Started watching SSH config');
		} catch (error) {
			this.logService.error('[SSHConfigService] Failed to start watching SSH config', error);
		}
	}

	stopWatching(): void {
		if (this._unwatchConfig) {
			this._unwatchConfig();
			this._unwatchConfig = undefined;
			this._isWatching = false;
			this.logService.info('[SSHConfigService] Stopped watching SSH config');
		}
	}

	convertToLabonairHost(systemHost: IHost): IHost {
		// Create a new host with a new ID, removing the system group
		const labonairHost: IHost = {
			...systemHost,
			id: generateUuid(),
			group: undefined,
			tags: systemHost.tags?.filter(tag => tag !== 'ssh-config') || [],
			created: Date.now()
		};

		this.logService.info(`[SSHConfigService] Converted system host ${systemHost.name} to Labonair host`);
		return labonairHost;
	}
}
