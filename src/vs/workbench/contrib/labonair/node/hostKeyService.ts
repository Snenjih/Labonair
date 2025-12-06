/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { promises as fs } from 'fs';
import { homedir } from 'os';
import { join } from 'path';
import * as crypto from 'crypto';

export interface IHostKeyEntry {
	hostname: string;
	port: number;
	keyType: string;
	key: Buffer;
	isHashed: boolean;
}

export enum HostKeyVerificationResult {
	Accepted = 'accepted',
	Rejected = 'rejected',
	Unknown = 'unknown'
}

export interface IHostKeyVerificationService {
	/**
	 * Verifies a host key against the known_hosts file
	 */
	verifyHostKey(host: string, port: number, keyAlgo: string, key: Buffer): Promise<HostKeyVerificationResult>;

	/**
	 * Adds a host key to the known_hosts file
	 */
	addHostKey(host: string, port: number, keyAlgo: string, key: Buffer): Promise<void>;

	/**
	 * Removes a host key from the known_hosts file
	 */
	removeHostKey(host: string, port: number): Promise<void>;

	/**
	 * Gets all known host keys
	 */
	getAllHostKeys(): Promise<IHostKeyEntry[]>;
}

export class HostKeyService implements IHostKeyVerificationService {
	private readonly knownHostsPath: string;
	private readonly labonairKnownHostsPath: string;

	constructor() {
		// Use system's known_hosts as primary source
		this.knownHostsPath = join(homedir(), '.ssh', 'known_hosts');
		// Use Labonair's own known_hosts for hosts added via Labonair
		this.labonairKnownHostsPath = join(homedir(), '.labonair', 'known_hosts');
	}

	/**
	 * Verifies if a host key matches a known host key
	 */
	async verifyHostKey(host: string, port: number, keyAlgo: string, key: Buffer): Promise<HostKeyVerificationResult> {
		try {
			// Check both system and Labonair known_hosts
			const systemKeys = await this._readKnownHosts(this.knownHostsPath);
			const labonairKeys = await this._readKnownHosts(this.labonairKnownHostsPath);
			const allKeys = [...systemKeys, ...labonairKeys];

			for (const entry of allKeys) {
				// Check if this entry matches our host
				if (await this._matchesHost(entry, host, port)) {
					// Check if key type matches
					if (entry.keyType === keyAlgo) {
						// Compare keys
						if (entry.key.equals(key)) {
							return HostKeyVerificationResult.Accepted;
						} else {
							// Key mismatch - potential security issue!
							return HostKeyVerificationResult.Rejected;
						}
					}
				}
			}

			// Host not found in known_hosts
			return HostKeyVerificationResult.Unknown;
		} catch (error) {
			console.error('Error verifying host key:', error);
			return HostKeyVerificationResult.Unknown;
		}
	}

	/**
	 * Adds a host key to Labonair's known_hosts file
	 */
	async addHostKey(host: string, port: number, keyAlgo: string, key: Buffer): Promise<void> {
		try {
			// Ensure .labonair directory exists
			const labonairDir = join(homedir(), '.labonair');
			await fs.mkdir(labonairDir, { recursive: true });

			// Format: hostname keytype base64key
			const hostname = port === 22 ? host : `[${host}]:${port}`;
			const keyBase64 = key.toString('base64');
			const line = `${hostname} ${keyAlgo} ${keyBase64}\n`;

			// Append to file
			await fs.appendFile(this.labonairKnownHostsPath, line, 'utf-8');
		} catch (error) {
			console.error('Error adding host key:', error);
			throw error;
		}
	}

	/**
	 * Removes a host key from Labonair's known_hosts file
	 */
	async removeHostKey(host: string, port: number): Promise<void> {
		try {
			const entries = await this._readKnownHosts(this.labonairKnownHostsPath);
			const filteredEntries = entries.filter(entry => {
				return !(entry.hostname === host && entry.port === port);
			});

			// Rewrite the file
			const lines = filteredEntries.map(entry => {
				const hostname = entry.port === 22 ? entry.hostname : `[${entry.hostname}]:${entry.port}`;
				const keyBase64 = entry.key.toString('base64');
				return `${hostname} ${entry.keyType} ${keyBase64}`;
			});

			await fs.writeFile(this.labonairKnownHostsPath, lines.join('\n') + '\n', 'utf-8');
		} catch (error) {
			console.error('Error removing host key:', error);
			throw error;
		}
	}

	/**
	 * Gets all known host keys from both system and Labonair files
	 */
	async getAllHostKeys(): Promise<IHostKeyEntry[]> {
		try {
			const systemKeys = await this._readKnownHosts(this.knownHostsPath);
			const labonairKeys = await this._readKnownHosts(this.labonairKnownHostsPath);
			return [...systemKeys, ...labonairKeys];
		} catch (error) {
			console.error('Error getting all host keys:', error);
			return [];
		}
	}

	/**
	 * Reads and parses a known_hosts file
	 */
	private async _readKnownHosts(filePath: string): Promise<IHostKeyEntry[]> {
		try {
			const content = await fs.readFile(filePath, 'utf-8');
			const lines = content.split('\n').filter(line => line.trim() && !line.startsWith('#'));
			const entries: IHostKeyEntry[] = [];

			for (const line of lines) {
				const entry = this._parseKnownHostsLine(line);
				if (entry) {
					entries.push(entry);
				}
			}

			return entries;
		} catch (error: any) {
			// File doesn't exist or can't be read
			if (error.code === 'ENOENT') {
				return [];
			}
			throw error;
		}
	}

	/**
	 * Parses a single line from known_hosts file
	 */
	private _parseKnownHostsLine(line: string): IHostKeyEntry | null {
		try {
			const parts = line.trim().split(/\s+/);
			if (parts.length < 3) {
				return null;
			}

			const [hostPart, keyType, keyBase64] = parts;

			// Check if hostname is hashed
			const isHashed = hostPart.startsWith('|1|');

			let hostname: string;
			let port = 22;

			if (isHashed) {
				// Hashed hostname - we'll store it as-is
				hostname = hostPart;
			} else {
				// Parse hostname and port
				const match = hostPart.match(/^\[([^\]]+)\]:(\d+)$/);
				if (match) {
					hostname = match[1];
					port = parseInt(match[2], 10);
				} else {
					hostname = hostPart;
				}
			}

			// Decode key
			const key = Buffer.from(keyBase64, 'base64');

			return {
				hostname,
				port,
				keyType,
				key,
				isHashed
			};
		} catch (error) {
			console.error('Error parsing known_hosts line:', error);
			return null;
		}
	}

	/**
	 * Checks if a known_hosts entry matches a given host and port
	 */
	private async _matchesHost(entry: IHostKeyEntry, host: string, port: number): Promise<boolean> {
		if (entry.isHashed) {
			// For hashed entries, we need to compute the hash and compare
			return this._matchesHashedHost(entry.hostname, host, port);
		} else {
			// Direct comparison
			return entry.hostname === host && entry.port === port;
		}
	}

	/**
	 * Checks if a hashed hostname matches the given host and port
	 * Format: |1|salt|hash where hash = HMAC-SHA1(salt, hostname)
	 */
	private _matchesHashedHost(hashedEntry: string, host: string, port: number): boolean {
		try {
			const parts = hashedEntry.split('|');
			if (parts.length !== 4 || parts[0] !== '' || parts[1] !== '1') {
				return false;
			}

			const salt = Buffer.from(parts[2], 'base64');
			const expectedHash = Buffer.from(parts[3], 'base64');

			// Compute hash for the given hostname
			const hostname = port === 22 ? host : `[${host}]:${port}`;
			const hmac = crypto.createHmac('sha1', salt);
			hmac.update(hostname);
			const computedHash = hmac.digest();

			return computedHash.equals(expectedHash);
		} catch (error) {
			console.error('Error matching hashed host:', error);
			return false;
		}
	}
}
