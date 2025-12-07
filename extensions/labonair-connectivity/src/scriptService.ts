/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import * as vscode from 'vscode';

export interface IScript {
	id: string;
	name: string;
	description?: string;
	content: string;
	tags?: string[];
	createdAt: number;
	updatedAt?: number;
}

const SCRIPTS_STORAGE_KEY = 'labonair.scripts';

export class ScriptService {
	private readonly _onDidChangeScripts = new vscode.EventEmitter<void>();
	readonly onDidChangeScripts: vscode.Event<void> = this._onDidChangeScripts.event;

	private _scripts: Map<string, IScript> = new Map();

	constructor(private readonly context: vscode.ExtensionContext) {
		this._loadScripts();
	}

	dispose(): void {
		this._onDidChangeScripts.dispose();
	}

	private _loadScripts(): void {
		try {
			const scriptsJson = this.context.globalState.get<string>(SCRIPTS_STORAGE_KEY);
			if (scriptsJson) {
				const scripts = JSON.parse(scriptsJson) as IScript[];
				scripts.forEach(script => {
					this._scripts.set(script.id, script);
				});
				console.log(`[ScriptService] Loaded ${scripts.length} scripts`);
			}
		} catch (error) {
			console.error('[ScriptService] Failed to load scripts', error);
		}
	}

	private async _saveScripts(): Promise<void> {
		try {
			const scripts = Array.from(this._scripts.values());
			const scriptsJson = JSON.stringify(scripts);
			await this.context.globalState.update(SCRIPTS_STORAGE_KEY, scriptsJson);
			this._onDidChangeScripts.fire();
			console.log(`[ScriptService] Saved ${scripts.length} scripts`);
		} catch (error) {
			console.error('[ScriptService] Failed to save scripts', error);
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
			script.id = this._generateUuid();
		}
		if (!script.createdAt) {
			script.createdAt = Date.now();
		}
		script.updatedAt = Date.now();

		this._scripts.set(script.id, script);
		await this._saveScripts();
		console.log(`[ScriptService] Added script: ${script.name} (${script.id})`);
	}

	async updateScript(id: string, updates: Partial<IScript>): Promise<void> {
		const script = this._scripts.get(id);
		if (!script) {
			console.warn(`[ScriptService] Script not found: ${id}`);
			return;
		}

		const updatedScript = {
			...script,
			...updates,
			updatedAt: Date.now()
		};

		this._scripts.set(id, updatedScript);
		await this._saveScripts();
		console.log(`[ScriptService] Updated script: ${script.name} (${id})`);
	}

	async deleteScript(id: string): Promise<void> {
		const script = this._scripts.get(id);
		if (!script) {
			return;
		}

		this._scripts.delete(id);
		await this._saveScripts();
		console.log(`[ScriptService] Deleted script: ${script.name} (${id})`);
	}

	async executeScript(scriptId: string, hostId: string): Promise<void> {
		const script = this._scripts.get(scriptId);
		if (!script) {
			console.error(`[ScriptService] Script not found: ${scriptId}`);
			throw new Error('Script not found');
		}

		console.log(`[ScriptService] Executing script "${script.name}" on host ${hostId}`);
		// NOTE: Actual script execution will be implemented in Phase 3 (Connectivity)
	}

	private _generateUuid(): string {
		return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function (c) {
			const r = Math.random() * 16 | 0;
			const v = c === 'x' ? r : (r & 0x3 | 0x8);
			return v.toString(16);
		});
	}
}
