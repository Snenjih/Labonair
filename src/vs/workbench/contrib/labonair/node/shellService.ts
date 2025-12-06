/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { promises as fs } from 'fs';
import { exec } from 'child_process';
import { promisify } from 'util';
import { platform } from 'os';
import { existsSync } from 'fs';

const execAsync = promisify(exec);

export interface IShellInfo {
	id: string;
	name: string;
	path: string;
	type: 'local' | 'wsl';
	description?: string;
}

export interface IShellDiscoveryService {
	/**
	 * Discovers all available shells on the system
	 */
	discoverShells(): Promise<IShellInfo[]>;

	/**
	 * Discovers WSL distributions (Windows only)
	 */
	discoverWSLDistributions(): Promise<IShellInfo[]>;

	/**
	 * Checks if a shell exists at the given path
	 */
	shellExists(path: string): Promise<boolean>;
}

export class ShellService implements IShellDiscoveryService {
	/**
	 * Discovers all available local shells and WSL distributions
	 */
	async discoverShells(): Promise<IShellInfo[]> {
		const shells: IShellInfo[] = [];

		// Add local shells based on platform
		const localShells = await this._discoverLocalShells();
		shells.push(...localShells);

		// Add WSL distributions (Windows only)
		if (platform() === 'win32') {
			const wslDistros = await this.discoverWSLDistributions();
			shells.push(...wslDistros);
		}

		return shells;
	}

	/**
	 * Discovers WSL distributions on Windows
	 */
	async discoverWSLDistributions(): Promise<IShellInfo[]> {
		if (platform() !== 'win32') {
			return [];
		}

		try {
			const { stdout } = await execAsync('wsl --list --verbose', { encoding: 'utf-16le' });
			const lines = stdout.split('\n').filter(line => line.trim());

			const distributions: IShellInfo[] = [];

			// Skip header line
			for (let i = 1; i < lines.length; i++) {
				const line = lines[i].trim();
				if (!line) {
					continue;
				}

				// Parse WSL list output
				// Format: "  NAME            STATE           VERSION"
				// Example: "* Ubuntu          Running         2"
				const match = line.match(/^\*?\s*([^\s]+)\s+(\w+)\s+(\d+)/);
				if (match) {
					const name = match[1];
					const state = match[2];
					const version = match[3];

					distributions.push({
						id: `wsl:${name}`,
						name: `WSL: ${name}`,
						path: `wsl -d ${name}`,
						type: 'wsl',
						description: `WSL ${version} - ${state}`
					});
				}
			}

			return distributions;
		} catch (error) {
			// WSL not installed or command failed
			console.log('WSL not available:', error);
			return [];
		}
	}

	/**
	 * Checks if a shell exists at the given path
	 */
	async shellExists(path: string): Promise<boolean> {
		try {
			return existsSync(path);
		} catch {
			return false;
		}
	}

	/**
	 * Discovers local shells based on the platform
	 */
	private async _discoverLocalShells(): Promise<IShellInfo[]> {
		const currentPlatform = platform();

		switch (currentPlatform) {
			case 'win32':
				return this._discoverWindowsShells();
			case 'darwin':
			case 'linux':
				return this._discoverUnixShells();
			default:
				return [];
		}
	}

	/**
	 * Discovers shells on Windows
	 */
	private async _discoverWindowsShells(): Promise<IShellInfo[]> {
		const shells: IShellInfo[] = [];

		// Common shell paths on Windows
		const shellPaths = [
			{
				id: 'cmd',
				name: 'Command Prompt',
				path: 'C:\\Windows\\System32\\cmd.exe',
				description: 'Windows Command Prompt'
			},
			{
				id: 'powershell',
				name: 'Windows PowerShell',
				path: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe',
				description: 'Windows PowerShell 5.1'
			},
			{
				id: 'pwsh',
				name: 'PowerShell Core',
				path: 'C:\\Program Files\\PowerShell\\7\\pwsh.exe',
				description: 'PowerShell 7+'
			},
			{
				id: 'git-bash',
				name: 'Git Bash',
				path: 'C:\\Program Files\\Git\\bin\\bash.exe',
				description: 'Git Bash (MinGW)'
			}
		];

		// Check which shells exist
		for (const shell of shellPaths) {
			if (existsSync(shell.path)) {
				shells.push({
					...shell,
					type: 'local'
				});
			}
		}

		// Try to find PowerShell Core via PATH
		try {
			const { stdout } = await execAsync('where pwsh');
			const pwshPath = stdout.trim().split('\n')[0];
			if (pwshPath && !shells.find(s => s.id === 'pwsh')) {
				shells.push({
					id: 'pwsh',
					name: 'PowerShell Core',
					path: pwshPath,
					type: 'local',
					description: 'PowerShell 7+'
				});
			}
		} catch {
			// pwsh not in PATH
		}

		return shells;
	}

	/**
	 * Discovers shells on Unix-like systems (macOS, Linux)
	 */
	private async _discoverUnixShells(): Promise<IShellInfo[]> {
		const shells: IShellInfo[] = [];

		try {
			// Read /etc/shells to get list of valid shells
			const shellsFile = await fs.readFile('/etc/shells', 'utf-8');
			const shellPaths = shellsFile
				.split('\n')
				.map(line => line.trim())
				.filter(line => line && !line.startsWith('#'));

			// Common shell information
			const shellInfo: Record<string, { name: string; description: string }> = {
				'bash': { name: 'Bash', description: 'Bourne Again Shell' },
				'zsh': { name: 'Zsh', description: 'Z Shell' },
				'fish': { name: 'Fish', description: 'Friendly Interactive Shell' },
				'sh': { name: 'sh', description: 'Bourne Shell' },
				'dash': { name: 'Dash', description: 'Debian Almquist Shell' },
				'ksh': { name: 'Ksh', description: 'Korn Shell' },
				'tcsh': { name: 'tcsh', description: 'TENEX C Shell' },
				'csh': { name: 'csh', description: 'C Shell' }
			};

			// Process each shell path
			for (const path of shellPaths) {
				if (!existsSync(path)) {
					continue;
				}

				// Extract shell name from path
				const shellName = path.split('/').pop() || path;
				const info = shellInfo[shellName] || { name: shellName, description: shellName };

				shells.push({
					id: `local:${shellName}:${path}`,
					name: info.name,
					path: path,
					type: 'local',
					description: `${info.description} (${path})`
				});
			}
		} catch (error) {
			console.error('Error reading /etc/shells:', error);

			// Fallback: add common shells if they exist
			const commonShells = [
				{ path: '/bin/bash', name: 'Bash', description: 'Bourne Again Shell' },
				{ path: '/bin/zsh', name: 'Zsh', description: 'Z Shell' },
				{ path: '/usr/bin/fish', name: 'Fish', description: 'Friendly Interactive Shell' },
				{ path: '/bin/sh', name: 'sh', description: 'Bourne Shell' }
			];

			for (const shell of commonShells) {
				if (existsSync(shell.path)) {
					const shellName = shell.path.split('/').pop() || shell.path;
					shells.push({
						id: `local:${shellName}:${shell.path}`,
						name: shell.name,
						path: shell.path,
						type: 'local',
						description: `${shell.description} (${shell.path})`
					});
				}
			}
		}

		return shells;
	}
}
