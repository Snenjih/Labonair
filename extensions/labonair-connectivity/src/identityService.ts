/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import * as vscode from 'vscode';

export type IdentityType = 'ssh-key' | 'password';

export interface IIdentity {
	id: string;
	name: string;
	type: IdentityType;
	publicKey?: string;
	comment?: string;
	createdAt: number;
}

const IDENTITIES_STORAGE_KEY = 'labonair.identities';
const SECRET_KEY_PREFIX = 'labonair.identity.';
const SECRET_PASSPHRASE_PREFIX = 'labonair.identity.passphrase.';

export class IdentityService {
	private readonly _onDidChangeIdentities = new vscode.EventEmitter<void>();
	readonly onDidChangeIdentities: vscode.Event<void> = this._onDidChangeIdentities.event;

	private _identities: Map<string, IIdentity> = new Map();

	constructor(private readonly context: vscode.ExtensionContext) {
		this._loadIdentities();
	}

	dispose(): void {
		this._onDidChangeIdentities.dispose();
	}

	private _loadIdentities(): void {
		try {
			const identitiesJson = this.context.globalState.get<string>(IDENTITIES_STORAGE_KEY);
			if (identitiesJson) {
				const identities = JSON.parse(identitiesJson) as IIdentity[];
				identities.forEach(identity => {
					this._identities.set(identity.id, identity);
				});
				console.log(`[IdentityService] Loaded ${identities.length} identities`);
			}
		} catch (error) {
			console.error('[IdentityService] Failed to load identities', error);
		}
	}

	private async _saveIdentities(): Promise<void> {
		try {
			const identities = Array.from(this._identities.values());
			const identitiesJson = JSON.stringify(identities);
			await this.context.globalState.update(IDENTITIES_STORAGE_KEY, identitiesJson);
			this._onDidChangeIdentities.fire();
			console.log(`[IdentityService] Saved ${identities.length} identities`);
		} catch (error) {
			console.error('[IdentityService] Failed to save identities', error);
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
			identity.id = this._generateUuid();
		}

		this._identities.set(identity.id, identity);

		const secretKey = `${SECRET_KEY_PREFIX}${identity.id}`;
		await this.context.secrets.store(secretKey, privateData);

		if (passphrase) {
			const passphraseKey = `${SECRET_PASSPHRASE_PREFIX}${identity.id}`;
			await this.context.secrets.store(passphraseKey, passphrase);
		}

		await this._saveIdentities();
		console.log(`[IdentityService] Added identity: ${identity.name} (${identity.id})`);
	}

	async updateIdentity(id: string, updates: Partial<IIdentity>): Promise<void> {
		const identity = this._identities.get(id);
		if (!identity) {
			console.warn(`[IdentityService] Identity not found: ${id}`);
			return;
		}

		const updatedIdentity = { ...identity, ...updates };
		this._identities.set(id, updatedIdentity);
		await this._saveIdentities();
		console.log(`[IdentityService] Updated identity: ${identity.name} (${id})`);
	}

	async deleteIdentity(id: string): Promise<void> {
		const identity = this._identities.get(id);
		if (!identity) {
			return;
		}

		this._identities.delete(id);

		const secretKey = `${SECRET_KEY_PREFIX}${identity.id}`;
		await this.context.secrets.delete(secretKey);

		const passphraseKey = `${SECRET_PASSPHRASE_PREFIX}${identity.id}`;
		await this.context.secrets.delete(passphraseKey);

		await this._saveIdentities();
		console.log(`[IdentityService] Deleted identity: ${identity.name} (${identity.id})`);
	}

	async getPrivateKey(id: string): Promise<string | undefined> {
		const secretKey = `${SECRET_KEY_PREFIX}${id}`;
		return await this.context.secrets.get(secretKey);
	}

	async getPassword(id: string): Promise<string | undefined> {
		const secretKey = `${SECRET_KEY_PREFIX}${id}`;
		return await this.context.secrets.get(secretKey);
	}

	private _generateUuid(): string {
		return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function (c) {
			const r = Math.random() * 16 | 0;
			const v = c === 'x' ? r : (r & 0x3 | 0x8);
			return v.toString(16);
		});
	}
}
