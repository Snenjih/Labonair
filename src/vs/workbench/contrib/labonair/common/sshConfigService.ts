/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { createDecorator } from '../../../../platform/instantiation/common/instantiation.js';
import { Event } from '../../../../base/common/event.js';
import { IHost } from './hostService.js';

export const ISSHConfigService = createDecorator<ISSHConfigService>('sshConfigService');

/**
 * Service for managing SSH config file integration.
 * Parses the system SSH config file and exposes entries as read-only hosts.
 */
export interface ISSHConfigService {
	readonly _serviceBrand: undefined;

	/**
	 * Fired when the SSH config file changes
	 */
	readonly onDidChangeConfig: Event<void>;

	/**
	 * Get all hosts from the SSH config file
	 */
	getSystemHosts(): Promise<IHost[]>;

	/**
	 * Get a specific host from the SSH config
	 */
	getSystemHost(alias: string): Promise<IHost | undefined>;

	/**
	 * Start watching the SSH config file for changes
	 */
	startWatching(): Promise<void>;

	/**
	 * Stop watching the SSH config file
	 */
	stopWatching(): void;

	/**
	 * Convert a system host to a Labonair host (detach/duplicate)
	 */
	convertToLabonairHost(systemHost: IHost): IHost;
}
