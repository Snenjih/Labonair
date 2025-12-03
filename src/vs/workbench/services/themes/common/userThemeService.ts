/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { createDecorator } from '../../../../platform/instantiation/common/instantiation.js';
import { Event } from '../../../../base/common/event.js';

export const IUserThemeService = createDecorator<IUserThemeService>('userThemeService');

export interface ITokenColor {
	name?: string;
	scope?: string | string[];
	settings: {
		foreground?: string;
		background?: string;
		fontStyle?: string;
	};
}

export interface IUserTheme {
	id: string;
	name: string;
	type: 'dark' | 'light' | 'hc';
	baseTheme?: string;
	colors: { [key: string]: string };
	tokenColors: ITokenColor[];
	iconTheme?: string;
	productIconTheme?: string;
	metadata: {
		createdAt: number;
		updatedAt: number;
		author?: string;
	};
}

export interface IUserThemeService {
	readonly _serviceBrand: undefined;

	/**
	 * Event fired when user themes change
	 */
	readonly onDidChangeUserThemes: Event<void>;

	/**
	 * Get all user themes
	 */
	getUserThemes(): Promise<IUserTheme[]>;

	/**
	 * Get a specific user theme by ID
	 */
	getUserTheme(id: string): Promise<IUserTheme | undefined>;

	/**
	 * Create a new user theme
	 */
	createUserTheme(theme: Omit<IUserTheme, 'id' | 'metadata'>): Promise<IUserTheme>;

	/**
	 * Update an existing user theme
	 */
	updateUserTheme(id: string, theme: Partial<IUserTheme>): Promise<IUserTheme>;

	/**
	 * Delete a user theme
	 */
	deleteUserTheme(id: string): Promise<void>;

	/**
	 * Export a theme as JSON string
	 */
	exportTheme(id: string): Promise<string>;

	/**
	 * Import a theme from JSON string
	 */
	importTheme(json: string): Promise<IUserTheme>;

	/**
	 * Get base theme colors
	 */
	getBaseThemeColors(baseThemeId: string): Promise<{ [key: string]: string }>;

	/**
	 * Generate theme preview image (base64 PNG)
	 */
	generateThemePreview(theme: IUserTheme): Promise<string>;
}
