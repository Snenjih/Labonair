/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Disposable } from '../../../../base/common/lifecycle.js';
import { IUserThemeService, IUserTheme } from '../common/userThemeService.js';
import { IFileService } from '../../../../platform/files/common/files.js';
import { ILogService } from '../../../../platform/log/common/log.js';
import { Emitter, Event } from '../../../../base/common/event.js';
import { URI } from '../../../../base/common/uri.js';
import { VSBuffer } from '../../../../base/common/buffer.js';
import { generateUuid } from '../../../../base/common/uuid.js';
import { IWorkbenchThemeService } from '../common/workbenchThemeService.js';
import { joinPath } from '../../../../base/common/resources.js';
import { registerSingleton, InstantiationType } from '../../../../platform/instantiation/common/extensions.js';
import { IBrowserWorkbenchEnvironmentService } from '../../environment/browser/environmentService.js';

export class UserThemeService extends Disposable implements IUserThemeService {
	declare readonly _serviceBrand: undefined;

	private readonly _onDidChangeUserThemes = this._register(new Emitter<void>());
	readonly onDidChangeUserThemes: Event<void> = this._onDidChangeUserThemes.event;

	private readonly userThemesPath: URI;

	constructor(
		@IFileService private readonly fileService: IFileService,
		@IBrowserWorkbenchEnvironmentService private readonly environmentService: IBrowserWorkbenchEnvironmentService,
		@ILogService private readonly logService: ILogService,
		@IWorkbenchThemeService private readonly themeService: IWorkbenchThemeService
	) {
		super();
		this.userThemesPath = joinPath(this.environmentService.userRoamingDataHome, '.labonair', 'user-themes');
		this._ensureUserThemesDirectory();
	}

	private async _ensureUserThemesDirectory(): Promise<void> {
		try {
			await this.fileService.createFolder(this.userThemesPath);
		} catch (error) {
			// Directory might already exist, which is fine
			this.logService.info('User themes directory setup', error);
		}
	}

	async getUserThemes(): Promise<IUserTheme[]> {
		try {
			const stat = await this.fileService.resolve(this.userThemesPath);
			if (!stat.children) {
				return [];
			}

			const themes: IUserTheme[] = [];
			for (const child of stat.children) {
				if (child.name.endsWith('.json') && !child.isDirectory) {
					try {
						const content = await this.fileService.readFile(child.resource);
						const theme = JSON.parse(content.value.toString()) as IUserTheme;
						themes.push(theme);
					} catch (error) {
						this.logService.warn(`Failed to load user theme ${child.name}`, error);
					}
				}
			}

			return themes;
		} catch (error) {
			this.logService.error('Failed to load user themes', error);
			return [];
		}
	}

	async getUserTheme(id: string): Promise<IUserTheme | undefined> {
		try {
			const themePath = joinPath(this.userThemesPath, `${id}.json`);
			const content = await this.fileService.readFile(themePath);
			return JSON.parse(content.value.toString()) as IUserTheme;
		} catch (error) {
			this.logService.warn(`Failed to load user theme ${id}`, error);
			return undefined;
		}
	}

	async createUserTheme(theme: Omit<IUserTheme, 'id' | 'metadata'>): Promise<IUserTheme> {
		const id = generateUuid();
		const now = Date.now();
		const newTheme: IUserTheme = {
			...theme,
			id,
			metadata: {
				createdAt: now,
				updatedAt: now,
				author: 'User'
			}
		};

		const themePath = joinPath(this.userThemesPath, `${id}.json`);
		const content = JSON.stringify(newTheme, null, 2);
		await this.fileService.writeFile(themePath, VSBuffer.fromString(content));

		this._onDidChangeUserThemes.fire();
		return newTheme;
	}

	async updateUserTheme(id: string, updates: Partial<IUserTheme>): Promise<IUserTheme> {
		const existingTheme = await this.getUserTheme(id);
		if (!existingTheme) {
			throw new Error(`Theme with id ${id} not found`);
		}

		const updatedTheme: IUserTheme = {
			...existingTheme,
			...updates,
			id: existingTheme.id, // Ensure ID doesn't change
			metadata: {
				...existingTheme.metadata,
				...updates.metadata,
				updatedAt: Date.now()
			}
		};

		const themePath = joinPath(this.userThemesPath, `${id}.json`);
		const content = JSON.stringify(updatedTheme, null, 2);
		await this.fileService.writeFile(themePath, VSBuffer.fromString(content));

		this._onDidChangeUserThemes.fire();
		return updatedTheme;
	}

	async deleteUserTheme(id: string): Promise<void> {
		const themePath = joinPath(this.userThemesPath, `${id}.json`);
		await this.fileService.del(themePath);
		this._onDidChangeUserThemes.fire();
	}

	async exportTheme(id: string): Promise<string> {
		const theme = await this.getUserTheme(id);
		if (!theme) {
			throw new Error(`Theme with id ${id} not found`);
		}
		return JSON.stringify(theme, null, 2);
	}

	async importTheme(json: string): Promise<IUserTheme> {
		const theme = JSON.parse(json) as Partial<IUserTheme>;

		// Validate required fields
		if (!theme.name || !theme.type || !theme.colors) {
			throw new Error('Invalid theme format: missing required fields (name, type, colors)');
		}

		// Validate theme type
		if (!['dark', 'light', 'hc'].includes(theme.type)) {
			throw new Error('Invalid theme type: must be dark, light, or hc');
		}

		// Create new theme from imported data
		return this.createUserTheme({
			name: theme.name,
			type: theme.type,
			baseTheme: theme.baseTheme,
			colors: theme.colors,
			tokenColors: theme.tokenColors || [],
			iconTheme: theme.iconTheme,
			productIconTheme: theme.productIconTheme
		});
	}

	async getBaseThemeColors(baseThemeId: string): Promise<{ [key: string]: string }> {
		// Get all available themes
		const themes = await this.themeService.getColorThemes();
		const baseTheme = themes.find(t => t.id === baseThemeId || t.settingsId === baseThemeId);

		if (!baseTheme) {
			this.logService.warn(`Base theme ${baseThemeId} not found`);
			return {};
		}

		// Extract colors from the theme
		const colors: { [key: string]: string } = {};
		const colorMap = (baseTheme as any).colorMap;
		if (colorMap) {
			for (const [key, value] of Object.entries(colorMap)) {
				if (value && typeof value === 'object' && 'toString' in value) {
					colors[key] = (value as any).toString();
				}
			}
		}

		return colors;
	}

	async generateThemePreview(theme: IUserTheme): Promise<string> {
		// For now, return a placeholder data URL
		// In a full implementation, this would generate an actual preview image
		// using canvas and rendering a mini-editor
		return 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
	}
}

registerSingleton(IUserThemeService, UserThemeService, InstantiationType.Delayed);

