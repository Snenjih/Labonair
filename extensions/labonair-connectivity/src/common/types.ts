export interface Tunnel {
	type: 'local' | 'remote';
	srcPort: number;
	dstHost: string;
	dstPort: number;
}

export interface Host {
	id: string;
	name: string;
	group: string;
	username: string;
	host: string;
	port: number;
	osIcon: 'linux' | 'windows' | 'mac' | 'docker' | 'other';
	tags: string[];
	jumpHostId?: string;
	tunnels?: Tunnel[];
	notes?: string;
	keepAlive?: boolean;
}

export interface Credential {
	id: string;
	name: string;
	username: string;
	type: 'password' | 'key';
}

export interface WebviewState {
	view: 'list' | 'edit' | 'credentials';
	hosts: Host[];
	selectedHost: Host | null;
}

export type Message =
	| { command: 'FETCH_DATA' }
	| { command: 'UPDATE_DATA', payload: { hosts: Host[] } }
	| { command: 'SAVE_HOST', payload: { host: Host, password?: string, keyPath?: string } }
	| { command: 'DELETE_HOST', payload: { id: string } }
	| { command: 'CONNECT_SSH', payload: { id: string } }
	| { command: 'PICK_KEY_FILE' }
	| { command: 'KEY_FILE_PICKED', payload: { path: string } }
	| { command: 'IMPORT_REQUEST', payload: { format: 'json' | 'ssh-config' } }
	| { command: 'EXPORT_REQUEST' };
