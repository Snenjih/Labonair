/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Disposable } from '../../../../base/common/lifecycle.js';
import { Emitter, Event } from '../../../../base/common/event.js';
import { IStorageService, StorageScope, StorageTarget } from '../../../../platform/storage/common/storage.js';
import { ILogService } from '../../../../platform/log/common/log.js';
import { IScript, IScriptService } from '../common/scriptService.js';
import { generateUuid } from '../../../../base/common/uuid.js';

const SCRIPTS_STORAGE_KEY = 'labonair.scripts';

export class ScriptService extends Disposable implements IScriptService {
	declare readonly _serviceBrand: undefined;

	private readonly _onDidChangeScripts = this._register(new Emitter<void>());
	readonly onDidChangeScripts: Event<void> = this._onDidChangeScripts.event;

	private _scripts: Map<string, IScript> = new Map();

	constructor(
		@IStorageService private readonly storageService: IStorageService,
		@ILogService private readonly logService: ILogService
	) {
		super();
		this._loadScripts();
	}

	private _loadScripts(): void {
		try {
			const scriptsJson = this.storageService.get(SCRIPTS_STORAGE_KEY, StorageScope.APPLICATION);
			if (scriptsJson) {
				const scripts = JSON.parse(scriptsJson) as IScript[];
				scripts.forEach(script => {
					this._scripts.set(script.id, script);
				});
				this.logService.info(`[ScriptService] Loaded ${scripts.length} scripts`);
			}
		} catch (error) {
			this.logService.error('[ScriptService] Failed to load scripts', error);
		}
	}

	private async _saveScripts(): Promise<void> {
		try {
			const scripts = Array.from(this._scripts.values());
			const scriptsJson = JSON.stringify(scripts);
			this.storageService.store(SCRIPTS_STORAGE_KEY, scriptsJson, StorageScope.APPLICATION, StorageTarget.USER);
			this._onDidChangeScripts.fire();
			this.logService.info(`[ScriptService] Saved ${scripts.length} scripts`);
		} catch (error) {
			this.logService.error('[ScriptService] Failed to save scripts', error);
		}
	}

	async getAllScripts(): Promise<IScript[]> {
		return Array.from(this._scripts.values());
	}

	async getScript(id: string): Promise<IScript | undefined> {
		return this._scripts.get(id);
	}

	async addScript(script: IScript): Promise<void> {
		if (!script.id) {
			script.id = generateUuid();
		}
		if (!script.createdAt) {
			script.createdAt = Date.now();
		}
		script.updatedAt = Date.now();

		this._scripts.set(script.id, script);
		await this._saveScripts();
		this.logService.info(`[ScriptService] Added script: ${script.name} (${script.id})`);
	}

	async updateScript(id: string, updates: Partial<IScript>): Promise<void> {
		const script = this._scripts.get(id);
		if (!script) {
			this.logService.warn(`[ScriptService] Script not found: ${id}`);
			return;
		}

		const updatedScript = {
			...script,
			...updates,
			updatedAt: Date.now()
		};

		this._scripts.set(id, updatedScript);
		await this._saveScripts();
		this.logService.info(`[ScriptService] Updated script: ${script.name} (${id})`);
	}

	async deleteScript(id: string): Promise<void> {
		const script = this._scripts.get(id);
		if (!script) {
			return;
		}

		this._scripts.delete(id);
		await this._saveScripts();
		this.logService.info(`[ScriptService] Deleted script: ${script.name} (${id})`);
	}

	async executeScript(scriptId: string, hostId: string): Promise<void> {
		const script = this._scripts.get(scriptId);
		if (!script) {
			this.logService.error(`[ScriptService] Script not found: ${scriptId}`);
			throw new Error('Script not found');
		}

		this.logService.info(`[ScriptService] Executing script "${script.name}" on host ${hostId}`);
		// NOTE: Actual script execution will be implemented in Phase 3 (Connectivity)
		// For now, this is just a placeholder that logs the execution intent
		// In Phase 3, this will:
		// 1. Get the host configuration
		// 2. Establish SSH connection
		// 3. Execute the script content via SSH
		// 4. Return the output
	}
}
