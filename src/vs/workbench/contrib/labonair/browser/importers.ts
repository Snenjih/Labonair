/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { IHost } from '../common/hostService.js';

export interface ImportResult {
	hosts: IHost[];
	errors: string[];
}

/**
 * Browser stub for LabonairImporter
 * The actual implementation requires Node.js fs module
 */
export class LabonairImporter {
	static async import(_filePath: string): Promise<ImportResult> {
		throw new Error('Import functionality is not available in browser context. Please use the desktop application.');
	}
}

/**
 * Browser stub for FileZillaImporter
 * The actual implementation requires Node.js fs module
 */
export class FileZillaImporter {
	static getDefaultPath(): string | undefined {
		return undefined;
	}

	static async import(_filePath: string): Promise<ImportResult> {
		throw new Error('Import functionality is not available in browser context. Please use the desktop application.');
	}
}

/**
 * Browser stub for WinSCPImporter
 * The actual implementation requires Node.js fs module
 */
export class WinSCPImporter {
	static getDefaultPath(): string | undefined {
		return undefined;
	}

	static async import(_filePath: string): Promise<ImportResult> {
		throw new Error('Import functionality is not available in browser context. Please use the desktop application.');
	}
}

/**
 * Browser stub for PuTTYImporter
 * The actual implementation requires Node.js fs module
 */
export class PuTTYImporter {
	static async import(): Promise<ImportResult> {
		throw new Error('Import functionality is not available in browser context. Please use the desktop application.');
	}
}
