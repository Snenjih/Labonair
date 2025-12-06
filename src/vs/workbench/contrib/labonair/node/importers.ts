/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { IHost, HostProtocol, OSIcon } from '../common/hostService.js';
import { generateUuid } from '../../../../base/common/uuid.js';

export interface ImportResult {
	hosts: IHost[];
	errors: string[];
}

/**
 * Labonair Import - Reads .labhosts JSON format
 */
export class LabonairImporter {
	static async import(filePath: string): Promise<ImportResult> {
		const errors: string[] = [];
		const hosts: IHost[] = [];

		try {
			const content = fs.readFileSync(filePath, 'utf-8');
			const data = JSON.parse(content);

			if (Array.isArray(data)) {
				for (const hostData of data) {
					try {
						// Generate new IDs for imported hosts
						const host: IHost = {
							...hostData,
							id: generateUuid(),
							created: Date.now(),
							tags: [...(hostData.tags || []), 'imported']
						};
						hosts.push(host);
					} catch (err) {
						errors.push(`Failed to parse host: ${err instanceof Error ? err.message : String(err)}`);
					}
				}
			} else {
				errors.push('Invalid Labonair file format');
			}
		} catch (err) {
			errors.push(`Failed to read file: ${err instanceof Error ? err.message : String(err)}`);
		}

		return { hosts, errors };
	}
}

/**
 * FileZilla Import - Parses sitemanager.xml
 */
export class FileZillaImporter {
	static async import(filePath: string): Promise<ImportResult> {
		const errors: string[] = [];
		const hosts: IHost[] = [];

		try {
			const content = fs.readFileSync(filePath, 'utf-8');

			// Simple XML parsing for FileZilla format
			// Pattern: <Server> tags with <Host>, <Port>, <User>, <Pass> etc.
			const serverMatches = content.matchAll(/<Server>([\s\S]*?)<\/Server>/g);

			for (const match of serverMatches) {
				try {
					const serverXml = match[1];

					const hostMatch = serverXml.match(/<Host>(.*?)<\/Host>/);
					const portMatch = serverXml.match(/<Port>(.*?)<\/Port>/);
					const userMatch = serverXml.match(/<User>(.*?)<\/User>/);
					const protocolMatch = serverXml.match(/<Protocol>(.*?)<\/Protocol>/);

					if (hostMatch) {
						const host: IHost = {
							id: generateUuid(),
							name: hostMatch[1] || 'Imported Server',
							group: 'FileZilla Import',
							tags: ['imported', 'filezilla'],
							connection: {
								host: hostMatch[1],
								port: portMatch ? parseInt(portMatch[1], 10) : 21,
								username: userMatch ? userMatch[1] : '',
								protocol: (protocolMatch && protocolMatch[1] === '1') ? 'ssh' : 'ssh',
								osIcon: 'unknown' as OSIcon
							},
							auth: {
								type: 'password'
							},
							sftp: {
								design: 'explorer'
							},
							created: Date.now()
						};
						hosts.push(host);
					}
				} catch (err) {
					errors.push(`Failed to parse server entry: ${err instanceof Error ? err.message : String(err)}`);
				}
			}

			if (hosts.length === 0 && errors.length === 0) {
				errors.push('No valid servers found in FileZilla XML');
			}
		} catch (err) {
			errors.push(`Failed to read FileZilla file: ${err instanceof Error ? err.message : String(err)}`);
		}

		return { hosts, errors };
	}

	static getDefaultPath(): string | undefined {
		if (process.platform === 'win32') {
			return path.join(os.homedir(), 'AppData', 'Roaming', 'FileZilla', 'sitemanager.xml');
		} else if (process.platform === 'darwin') {
			return path.join(os.homedir(), '.config', 'filezilla', 'sitemanager.xml');
		} else {
			return path.join(os.homedir(), '.config', 'filezilla', 'sitemanager.xml');
		}
	}
}

/**
 * WinSCP Import - Parses WinSCP.ini
 */
export class WinSCPImporter {
	static async import(filePath: string): Promise<ImportResult> {
		const errors: string[] = [];
		const hosts: IHost[] = [];

		try {
			const content = fs.readFileSync(filePath, 'utf-8');
			const lines = content.split('\n');

			let currentSession: any = null;

			for (const line of lines) {
				const trimmed = line.trim();

				// Session section start
				if (trimmed.startsWith('[Sessions\\')) {
					if (currentSession) {
						hosts.push(this._convertToHost(currentSession));
					}
					const sessionName = trimmed.match(/\[Sessions\\(.*?)\]/);
					currentSession = {
						name: sessionName ? sessionName[1] : 'Unknown'
					};
				} else if (currentSession && trimmed.includes('=')) {
					const [key, value] = trimmed.split('=', 2);
					currentSession[key.trim()] = value.trim();
				}
			}

			// Add last session
			if (currentSession) {
				hosts.push(this._convertToHost(currentSession));
			}

			if (hosts.length === 0) {
				errors.push('No valid sessions found in WinSCP.ini');
			}
		} catch (err) {
			errors.push(`Failed to read WinSCP file: ${err instanceof Error ? err.message : String(err)}`);
		}

		return { hosts, errors };
	}

	private static _convertToHost(session: any): IHost {
		return {
			id: generateUuid(),
			name: session.name || 'WinSCP Import',
			group: 'WinSCP Import',
			tags: ['imported', 'winscp'],
			connection: {
				host: session.HostName || '',
				port: parseInt(session.PortNumber || '22', 10),
				username: session.UserName || '',
				protocol: 'ssh' as HostProtocol,
				osIcon: 'unknown' as OSIcon
			},
			auth: {
				type: session.PublicKeyFile ? 'key' : 'password'
			},
			sftp: {
				design: 'commander'
			},
			created: Date.now()
		};
	}

	static getDefaultPath(): string | undefined {
		if (process.platform === 'win32') {
			return path.join(os.homedir(), 'AppData', 'Roaming', 'WinSCP', 'WinSCP.ini');
		}
		return undefined;
	}
}

/**
 * PuTTY Import - Reads Windows Registry (Windows only)
 * Note: This is a simplified version. Full implementation would require Windows Registry access
 */
export class PuTTYImporter {
	static async import(): Promise<ImportResult> {
		const errors: string[] = [];
		const hosts: IHost[] = [];

		if (process.platform !== 'win32') {
			errors.push('PuTTY import is only available on Windows');
			return { hosts, errors };
		}

		try {
			// TODO: Implement Windows Registry reading
			// This would require using node-windows-registry or similar
			// For now, return placeholder
			errors.push('PuTTY import not yet fully implemented');
		} catch (err) {
			errors.push(`Failed to read PuTTY registry: ${err instanceof Error ? err.message : String(err)}`);
		}

		return { hosts, errors };
	}
}

/**
 * Generic SSH Config format exporter
 */
export class SSHConfigExporter {
	static export(hosts: IHost[]): string {
		let config = '# Generated by Labonair\n\n';

		for (const host of hosts) {
			if (host.connection.protocol !== 'ssh') {
				continue;
			}

			config += `Host ${host.name}\n`;
			config += `    HostName ${host.connection.host}\n`;
			config += `    Port ${host.connection.port}\n`;
			config += `    User ${host.connection.username}\n`;

			if (host.advanced?.jumpHostId) {
				config += `    ProxyJump ${host.advanced.jumpHostId}\n`;
			}

			if (host.advanced?.keepAliveInterval) {
				config += `    ServerAliveInterval ${host.advanced.keepAliveInterval}\n`;
			}

			config += '\n';
		}

		return config;
	}
}
