/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { IHost, HostProtocol, OSIcon } from '../common/hostService.js';
import { generateUuid } from '../../../../base/common/uuid.js';

export interface SSHConfigEntry {
	host: string;
	hostname?: string;
	port?: number;
	user?: string;
	identityFile?: string;
	proxyJump?: string;
	proxyCommand?: string;
}

export class SSHConfigParser {
	private static readonly DEFAULT_SSH_CONFIG_PATH = path.join(os.homedir(), '.ssh', 'config');

	static async parse(configPath?: string): Promise<IHost[]> {
		const filePath = configPath || this.DEFAULT_SSH_CONFIG_PATH;

		if (!fs.existsSync(filePath)) {
			return [];
		}

		const content = fs.readFileSync(filePath, 'utf-8');
		const entries = this._parseContent(content);

		return entries.map(entry => this._convertToHost(entry));
	}

	private static _parseContent(content: string): SSHConfigEntry[] {
		const entries: SSHConfigEntry[] = [];
		let currentEntry: SSHConfigEntry | null = null;

		const lines = content.split('\n');

		for (const line of lines) {
			const trimmed = line.trim();

			if (trimmed === '' || trimmed.startsWith('#')) {
				continue;
			}

			const match = trimmed.match(/^(\w+)\s+(.+)$/);
			if (!match) {
				continue;
			}

			const [, key, value] = match;
			const lowerKey = key.toLowerCase();

			if (lowerKey === 'host') {
				if (currentEntry && currentEntry.hostname) {
					entries.push(currentEntry);
				}

				if (value !== '*' && !value.includes('*')) {
					currentEntry = { host: value };
				} else {
					currentEntry = null;
				}
			} else if (currentEntry) {
				switch (lowerKey) {
					case 'hostname':
						currentEntry.hostname = value;
						break;
					case 'port':
						currentEntry.port = parseInt(value, 10);
						break;
					case 'user':
						currentEntry.user = value;
						break;
					case 'identityfile':
						currentEntry.identityFile = value.replace('~', os.homedir());
						break;
					case 'proxyjump':
						currentEntry.proxyJump = value;
						break;
					case 'proxycommand':
						currentEntry.proxyCommand = value;
						break;
				}
			}
		}

		if (currentEntry && currentEntry.hostname) {
			entries.push(currentEntry);
		}

		return entries;
	}

	private static _convertToHost(entry: SSHConfigEntry): IHost {
		return {
			id: generateUuid(),
			name: entry.host,
			group: 'SSH Config',
			tags: ['imported', 'ssh-config'],
			connection: {
				host: entry.hostname || entry.host,
				port: entry.port || 22,
				username: entry.user || os.userInfo().username,
				osIcon: 'unknown' as OSIcon,
				protocol: 'ssh' as HostProtocol
			},
			auth: {
				type: entry.identityFile ? 'key' : 'agent'
			},
			advanced: {
				proxyCommand: entry.proxyCommand
			}
		};
	}

	static async watch(callback: (hosts: IHost[]) => void, configPath?: string): Promise<() => void> {
		const filePath = configPath || this.DEFAULT_SSH_CONFIG_PATH;

		if (!fs.existsSync(filePath)) {
			return () => { };
		}

		const watcher = fs.watch(filePath, async () => {
			const hosts = await this.parse(filePath);
			callback(hosts);
		});

		return () => {
			watcher.close();
		};
	}
}
