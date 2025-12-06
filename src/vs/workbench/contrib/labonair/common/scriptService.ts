/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { createDecorator } from '../../../../platform/instantiation/common/instantiation.js';
import { Event } from '../../../../base/common/event.js';

export const IScriptService = createDecorator<IScriptService>('scriptService');

export interface IScript {
	id: string;
	name: string;
	description?: string;
	content: string;
	tags?: string[];
	createdAt: number;
	updatedAt?: number;
}

export interface IScriptService {
	readonly _serviceBrand: undefined;

	readonly onDidChangeScripts: Event<void>;

	getAllScripts(): Promise<IScript[]>;
	getScript(id: string): Promise<IScript | undefined>;
	addScript(script: IScript): Promise<void>;
	updateScript(id: string, script: Partial<IScript>): Promise<void>;
	deleteScript(id: string): Promise<void>;

	executeScript(scriptId: string, hostId: string): Promise<void>;
}
