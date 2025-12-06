/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Disposable } from '../../../../base/common/dispose.js';
import { Emitter, Event } from '../../../../base/common/event.js';
import { IStorageService, StorageScope, StorageTarget } from '../../../../platform/storage/common/storage.js';
import { ISecretStorageService } from '../../../../platform/secrets/common/secrets.js';
import { ILogService } from '../../../../platform/log/common/log.js';
import { IIdentity, IIdentityService } from '../common/identityService.js';
import { generateUuid } from '../../../../base/common/uuid.js';

const IDENTITIES_STORAGE_KEY = 'labonair.identities';
const SECRET_KEY_PREFIX = 'labonair.identity.';
const SECRET_PASSPHRASE_PREFIX = 'labonair.identity.passphrase.';

export class IdentityService extends Disposable implements IIdentityService {
	declare readonly _serviceBrand: undefined;

	private readonly _onDidChangeIdentities = this._register(new Emitter<void>());
	readonly onDidChangeIdentities: Event<void> = this._onDidChangeIdentities.event;

	private _identities: Map<string, IIdentity> = new Map();

	constructor(
		@IStorageService private readonly storageService: IStorageService,
		@ISecretStorageService private readonly secretStorageService: ISecretStorageService,
		@ILogService private readonly logService: ILogService
	) {
		super();
		this._loadIdentities();
	}

	private _loadIdentities(): void {
		try {
			const identitiesJson = this.storageService.get(IDENTITIES_STORAGE_KEY, StorageScope.APPLICATION);
			if (identitiesJson) {
				const identities = JSON.parse(identitiesJson) as IIdentity[];
				identities.forEach(identity => {
					this._identities.set(identity.id, identity);
				});
				this.logService.info(`[IdentityService] Loaded ${identities.length} identities`);
			}
		} catch (error) {
			this.logService.error('[IdentityService] Failed to load identities', error);
		}
	}

	private async _saveIdentities(): Promise<void> {
		try {
			const identities = Array.from(this._identities.values());
			const identitiesJson = JSON.stringify(identities);
			this.storageService.store(IDENTITIES_STORAGE_KEY, identitiesJson, StorageScope.APPLICATION, StorageTarget.USER);
			this._onDidChangeIdentities.fire();
			this.logService.info(`[IdentityService] Saved ${identities.length} identities`);
		} catch (error) {
			this.logService.error('[IdentityService] Failed to save identities', error);
		}
	}

	async getAllIdentities(): Promise<IIdentity[]> {
		return Array.from(this._identities.values());
	}

	async getIdentity(id: string): Promise<IIdentity | undefined> {
		return this._identities.get(id);
	}

	async addIdentity(identity: IIdentity, privateData: string, passphrase?: string): Promise<void> {
		if (!identity.id) {
			identity.id = generateUuid();
		}

		this._identities.set(identity.id, identity);

		const secretKey = `${SECRET_KEY_PREFIX}${identity.id}`;
		await this.secretStorageService.set(secretKey, privateData);

		if (passphrase) {
			const passphraseKey = `${SECRET_PASSPHRASE_PREFIX}${identity.id}`;
			await this.secretStorageService.set(passphraseKey, passphrase);
		}

		await this._saveIdentities();
		this.logService.info(`[IdentityService] Added identity: ${identity.name} (${identity.id})`);
	}

	async updateIdentity(id: string, updates: Partial<IIdentity>): Promise<void> {
		const identity = this._identities.get(id);
		if (!identity) {
			this.logService.warn(`[IdentityService] Identity not found: ${id}`);
			return;
		}

		const updatedIdentity = { ...identity, ...updates };
		this._identities.set(id, updatedIdentity);
		await this._saveIdentities();
		this.logService.info(`[IdentityService] Updated identity: ${identity.name} (${id})`);
	}

	async deleteIdentity(id: string): Promise<void> {
		const identity = this._identities.get(id);
		if (!identity) {
			return;
		}

		this._identities.delete(id);

		const secretKey = `${SECRET_KEY_PREFIX}${identity.id}`;
		await this.secretStorageService.delete(secretKey);

		const passphraseKey = `${SECRET_PASSPHRASE_PREFIX}${identity.id}`;
		await this.secretStorageService.delete(passphraseKey);

		await this._saveIdentities();
		this.logService.info(`[IdentityService] Deleted identity: ${identity.name} (${id})`);
	}

	async getPrivateKey(id: string): Promise<string | undefined> {
		const secretKey = `${SECRET_KEY_PREFIX}${id}`;
		return await this.secretStorageService.get(secretKey);
	}

	async getPassword(id: string): Promise<string | undefined> {
		const secretKey = `${SECRET_KEY_PREFIX}${id}`;
		return await this.secretStorageService.get(secretKey);
	}
}
