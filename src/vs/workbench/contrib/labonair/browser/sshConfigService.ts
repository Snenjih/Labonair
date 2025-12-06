/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Disposable } from '../../../../base/common/lifecycle.js';
import { Emitter, Event } from '../../../../base/common/event.js';
import { IHost } from '../common/hostService.js';
import { ISSHConfigService } from '../common/sshConfigService.js';
import { generateUuid } from '../../../../base/common/uuid.js';

/**
 * Browser implementation of SSH Config Service.
 * This is a no-op implementation since SSH config files cannot be accessed in browser context.
 */
export class SSHConfigService extends Disposable implements ISSHConfigService {
	declare readonly _serviceBrand: undefined;

	private readonly _onDidChangeConfig = this._register(new Emitter<void>());
	readonly onDidChangeConfig: Event<void> = this._onDidChangeConfig.event;

	async getSystemHosts(): Promise<IHost[]> {
		// SSH config is not available in browser context
		return [];
	}

	async getSystemHost(alias: string): Promise<IHost | undefined> {
		// SSH config is not available in browser context
		return undefined;
	}

	async startWatching(): Promise<void> {
		// No-op in browser context
	}

	stopWatching(): void {
		// No-op in browser context
	}

	convertToLabonairHost(systemHost: IHost): IHost {
		// Create a copy with a new ID
		return {
			...systemHost,
			id: generateUuid(),
			created: Date.now()
		};
	}
}
