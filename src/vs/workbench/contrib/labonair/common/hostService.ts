/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { createDecorator } from '../../../../platform/instantiation/common/instantiation.js';
import { Event } from '../../../../base/common/event.js';

export const IHostService = createDecorator<IHostService>('hostService');

export type HostProtocol = 'ssh' | 'local' | 'wsl';
export type AuthType = 'password' | 'key' | 'agent' | 'identity_ref';
export type OSIcon = 'linux' | 'windows' | 'macos' | 'freebsd' | 'unknown';
export type SFTPDesign = 'explorer' | 'commander';
export type HostStatus = 'online' | 'offline' | 'unknown' | 'connecting';

export interface IPortTunnel {
	localPort: number;
	remoteHost: string;
	remotePort: number;
}

export interface IHostAuth {
	type: AuthType;
	identityId?: string;
}

export interface IHostConnection {
	host: string;
	port: number;
	username: string;
	osIcon: OSIcon;
	protocol: HostProtocol;
}

export interface IHostAdvanced {
	jumpHostId?: string;
	proxyCommand?: string;
	keepAliveInterval?: number;
	encoding?: string;
	postExecScript?: string;
}

export interface IHostTerminal {
	cursorStyle?: string;
	blinking?: boolean;
	tabColor?: string;
	fontSize?: number;
}

export interface IHostSFTP {
	design: SFTPDesign;
	layout?: string;
	localPath?: string;
	remotePath?: string;
}

export interface IHost {
	id: string;
	name: string;
	group?: string;
	tags?: string[];
	connection: IHostConnection;
	auth: IHostAuth;
	advanced?: IHostAdvanced;
	tunnels?: IPortTunnel[];
	terminal?: IHostTerminal;
	sftp?: IHostSFTP;
	notes?: string;
	status?: HostStatus;
	lastUsed?: number;
}

export interface IHostGroup {
	name: string;
	defaults?: Partial<IHost>;
}

export interface IHostService {
	readonly _serviceBrand: undefined;

	readonly onDidChangeHosts: Event<void>;
	readonly onDidChangeStatus: Event<{ hostId: string; status: HostStatus }>;

	getAllHosts(): Promise<IHost[]>;
	getHost(id: string): Promise<IHost | undefined>;
	addHost(host: IHost): Promise<void>;
	updateHost(id: string, host: Partial<IHost>): Promise<void>;
	deleteHost(id: string): Promise<void>;

	getGroups(): Promise<IHostGroup[]>;
	addGroup(group: IHostGroup): Promise<void>;
	updateGroup(name: string, group: Partial<IHostGroup>): Promise<void>;
	deleteGroup(name: string): Promise<void>;

	getHostStatus(id: string): HostStatus;
	refreshStatus(id: string): Promise<void>;

	importHosts(source: 'ssh-config' | 'filezilla' | 'labonair', data: string): Promise<IHost[]>;
	exportHosts(hostIds: string[], encrypt?: boolean): Promise<string>;
}
