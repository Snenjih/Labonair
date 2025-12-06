/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { Disposable } from '../../../../base/common/lifecycle.js';
import { ILogService } from '../../../../platform/log/common/log.js';
import { platform, Platform } from '../../../../base/common/platform.js';
import { exec } from 'child_process';
import { promisify } from 'util';
import * as fs from 'fs';

const execAsync = promisify(exec);

export interface ISSHAgentService {
	readonly _serviceBrand: undefined;

	isAgentRunning(): Promise<boolean>;
	getAgentStatus(): Promise<{ running: boolean; socket?: string; pid?: number }>;
}

export class SSHAgentService extends Disposable implements ISSHAgentService {
	declare readonly _serviceBrand: undefined;

	constructor(
		@ILogService private readonly logService: ILogService
	) {
		super();
	}

	/**
	 * Checks if an SSH agent is running
	 * - On Unix/macOS: Checks for SSH_AUTH_SOCK environment variable and socket file existence
	 * - On Windows: Checks for Pageant process or OpenSSH Authentication Agent service
	 */
	async isAgentRunning(): Promise<boolean> {
		try {
			const status = await this.getAgentStatus();
			return status.running;
		} catch (error) {
			this.logService.error('[SSHAgentService] Error checking agent status:', error);
			return false;
		}
	}

	/**
	 * Gets detailed SSH agent status
	 */
	async getAgentStatus(): Promise<{ running: boolean; socket?: string; pid?: number }> {
		if (platform === Platform.Windows) {
			return this._getWindowsAgentStatus();
		} else {
			return this._getUnixAgentStatus();
		}
	}

	/**
	 * Checks for SSH agent on Unix/macOS systems
	 */
	private async _getUnixAgentStatus(): Promise<{ running: boolean; socket?: string; pid?: number }> {
		try {
			// Check SSH_AUTH_SOCK environment variable
			const authSock = process.env.SSH_AUTH_SOCK;

			if (!authSock) {
				this.logService.debug('[SSHAgentService] SSH_AUTH_SOCK not set');
				return { running: false };
			}

			// Check if the socket file exists
			if (!fs.existsSync(authSock)) {
				this.logService.debug('[SSHAgentService] SSH agent socket file does not exist:', authSock);
				return { running: false };
			}

			// Try to get agent PID
			let pid: number | undefined;
			const agentPid = process.env.SSH_AGENT_PID;
			if (agentPid) {
				pid = parseInt(agentPid, 10);
			}

			// Verify agent is responsive by listing keys (even if empty)
			try {
				await execAsync('ssh-add -l', { timeout: 2000 });
				this.logService.info(`[SSHAgentService] SSH agent is running (socket: ${authSock})`);
				return { running: true, socket: authSock, pid };
			} catch (error: any) {
				// Exit code 1 means "no identities", which is still a running agent
				// Exit code 2 means "could not connect to agent"
				if (error.code === 1) {
					this.logService.info(`[SSHAgentService] SSH agent is running with no identities (socket: ${authSock})`);
					return { running: true, socket: authSock, pid };
				}
				this.logService.debug('[SSHAgentService] SSH agent not responsive:', error.message);
				return { running: false };
			}
		} catch (error) {
			this.logService.error('[SSHAgentService] Error checking Unix SSH agent:', error);
			return { running: false };
		}
	}

	/**
	 * Checks for SSH agent on Windows systems
	 * Looks for:
	 * 1. Pageant (PuTTY agent) process
	 * 2. OpenSSH Authentication Agent service
	 */
	private async _getWindowsAgentStatus(): Promise<{ running: boolean; socket?: string; pid?: number }> {
		try {
			// Check for Pageant process
			try {
				const { stdout } = await execAsync('tasklist /FI "IMAGENAME eq pageant.exe" /NH', { timeout: 2000 });
				if (stdout.toLowerCase().includes('pageant.exe')) {
					this.logService.info('[SSHAgentService] Pageant is running');
					return { running: true, socket: 'pageant' };
				}
			} catch (error) {
				this.logService.debug('[SSHAgentService] Pageant not found');
			}

			// Check for OpenSSH Authentication Agent service
			try {
				const { stdout } = await execAsync('sc query ssh-agent', { timeout: 2000 });
				if (stdout.includes('RUNNING')) {
					this.logService.info('[SSHAgentService] OpenSSH Authentication Agent is running');
					return { running: true, socket: 'openssh-agent' };
				}
			} catch (error) {
				this.logService.debug('[SSHAgentService] OpenSSH Authentication Agent not running');
			}

			// Check SSH_AUTH_SOCK for Windows OpenSSH
			const authSock = process.env.SSH_AUTH_SOCK;
			if (authSock) {
				try {
					// On Windows, ssh-add might be available with Git Bash or WSL
					await execAsync('ssh-add -l', { timeout: 2000 });
					this.logService.info(`[SSHAgentService] SSH agent detected via SSH_AUTH_SOCK: ${authSock}`);
					return { running: true, socket: authSock };
				} catch (error: any) {
					if (error.code === 1) {
						this.logService.info(`[SSHAgentService] SSH agent running with no identities (socket: ${authSock})`);
						return { running: true, socket: authSock };
					}
				}
			}

			this.logService.debug('[SSHAgentService] No SSH agent found on Windows');
			return { running: false };
		} catch (error) {
			this.logService.error('[SSHAgentService] Error checking Windows SSH agent:', error);
			return { running: false };
		}
	}
}
