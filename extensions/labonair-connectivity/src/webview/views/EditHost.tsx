import React, { useState, useEffect } from 'react';
import { Host, Tunnel, Message } from '../../common/types';
import { TagInput } from '../components/TagInput';
import { TunnelList } from '../components/TunnelList';
import vscode from '../utils/vscode';
import '../styles/forms.css';

interface EditHostProps {
	initialHost?: Host | null;
	agentAvailable?: boolean;
	onSave: (host: Host, password?: string, keyPath?: string) => void;
	onCancel: () => void;
}

const EditHost: React.FC<EditHostProps> = ({ initialHost, agentAvailable, onSave, onCancel }) => {
	const [activeTab, setActiveTab] = useState<'general' | 'auth' | 'advanced'>('general');

	// Form State
	const [name, setName] = useState(initialHost?.name || '');
	const [group, setGroup] = useState(initialHost?.group || '');
	const [host, setHost] = useState(initialHost?.host || '');
	const [port, setPort] = useState(initialHost?.port?.toString() || '22');
	const [username, setUsername] = useState(initialHost?.username || '');
	const [osIcon, setOsIcon] = useState<Host['osIcon']>(initialHost?.osIcon || 'linux');
	const [tags, setTags] = useState<string[]>(initialHost?.tags || []);
	const [jumpHostId, setJumpHostId] = useState(initialHost?.jumpHostId || '');

	const [tunnels, setTunnels] = useState<Tunnel[]>(initialHost?.tunnels || []);
	const [notes, setNotes] = useState(initialHost?.notes || '');
	const [keepAlive, setKeepAlive] = useState(initialHost?.keepAlive || false);

	// Auth State
	const [authType, setAuthType] = useState<'password' | 'key' | 'agent'>('key'); // Defaulting to key for now or could be inferred
	const [password, setPassword] = useState('');
	const [keyPath, setKeyPath] = useState('');

	useEffect(() => {
		const handleMessage = (event: MessageEvent) => {
			const message: Message = event.data;
			if (message.command === 'KEY_FILE_PICKED') {
				setKeyPath(message.payload.path);
			}
		};
		window.addEventListener('message', handleMessage);
		return () => window.removeEventListener('message', handleMessage);
	}, []);

	const handlePickKey = () => {
		vscode.postMessage({ command: 'PICK_KEY_FILE' });
	};

	const handleSubmit = (e: React.FormEvent) => {
		e.preventDefault();

		const newHost: Host = {
			id: initialHost?.id || crypto.randomUUID(),
			name,
			group,
			host,
			port: parseInt(port),
			username,
			osIcon,
			tags,
			jumpHostId: jumpHostId || undefined,
			tunnels: tunnels.length > 0 ? tunnels : undefined,
			notes: notes || undefined,
			keepAlive: keepAlive || undefined
		};

		onSave(newHost, password || undefined, keyPath || undefined);
	};

	return (
		<div className="edit-host-view">
			<h2>{initialHost ? 'Edit Host' : 'New Host'}</h2>

			<div className="tabs">
				<button
					className={`tab ${activeTab === 'general' ? 'active' : ''}`}
					onClick={() => setActiveTab('general')}
				>
					General
				</button>
				<button
					className={`tab ${activeTab === 'auth' ? 'active' : ''}`}
					onClick={() => setActiveTab('auth')}
				>
					Auth
				</button>
				<button
					className={`tab ${activeTab === 'advanced' ? 'active' : ''}`}
					onClick={() => setActiveTab('advanced')}
				>
					Advanced
				</button>
			</div>

			<form onSubmit={handleSubmit}>
				{activeTab === 'general' && (
					<div className="form-group">
						<label>Label</label>
						<input className="vscode-input" value={name} onChange={e => setName(e.target.value)} required />

						<label>Group</label>
						<input className="vscode-input" value={group} onChange={e => setGroup(e.target.value)} list="group-suggestions" />
						<datalist id="group-suggestions">
							<option value="Production" />
							<option value="Staging" />
							<option value="Development" />
						</datalist>

						<label>Host Address</label>
						<input className="vscode-input" value={host} onChange={e => setHost(e.target.value)} required />

						<label>Port</label>
						<input type="number" className="vscode-input" value={port} onChange={e => setPort(e.target.value)} />

						<label>Username</label>
						<input className="vscode-input" value={username} onChange={e => setUsername(e.target.value)} required />

						<label>Tags</label>
						<TagInput tags={tags} onChange={setTags} />

						<label>OS Icon</label>
						<select className="vscode-input" value={osIcon} onChange={e => setOsIcon(e.target.value as any)}>
							<option value="linux">Linux</option>
							<option value="windows">Windows</option>
							<option value="mac">macOS</option>
							<option value="docker">Docker</option>
							<option value="other">Other</option>
						</select>
					</div>
				)}

				{activeTab === 'auth' && (
					<div className="form-group">
						<label>Authentication Type</label>
						<div className="segmented-control">
							<button type="button" className={authType === 'password' ? 'active' : ''} onClick={() => setAuthType('password')}>Password</button>
							<button type="button" className={authType === 'key' ? 'active' : ''} onClick={() => setAuthType('key')}>Key File</button>
							<button type="button" className={authType === 'agent' ? 'active' : ''} onClick={() => setAuthType('agent')}>Agent</button>
						</div>

						{authType === 'password' && (
							<>
								<label>Password</label>
								<input type="password" className="vscode-input" value={password} onChange={e => setPassword(e.target.value)} placeholder={initialHost ? "Leave empty to keep unchanged" : ""} />
							</>
						)}

						{authType === 'key' && (
							<>
								<label>Private Key Path</label>
								<div style={{ display: 'flex', gap: '8px' }}>
									<input className="vscode-input" value={keyPath} onChange={e => setKeyPath(e.target.value)} placeholder="~/.ssh/id_rsa" />
									<button type="button" className="vscode-button secondary" onClick={handlePickKey}>Browse...</button>
								</div>
							</>
						)}
						{authType === 'agent' && (
							<div className="info-text">Using SSH Agent for authentication.</div>
						)}
					</div>
				)}

				{activeTab === 'advanced' && (
					<div className="form-group">
						<label>Tunnels (Port Forwarding)</label>
						<TunnelList tunnels={tunnels} onChange={setTunnels} />

						<label>Jump Host (Optional)</label>
						<input className="vscode-input" value={jumpHostId} onChange={e => setJumpHostId(e.target.value)} placeholder="Host ID" />

						<label>Keep Alive</label>
						<div className="checkbox-wrapper">
							<input type="checkbox" checked={keepAlive} onChange={e => setKeepAlive(e.target.checked)} />
							<span>Enable SSH KeepAlive</span>
						</div>

						<label>Notes</label>
						<textarea className="vscode-input" value={notes} onChange={e => setNotes(e.target.value)} rows={4} />
					</div>
				)}

				{authType === 'agent' && (
					<div className="form-info">
						{agentAvailable ? (
							<span style={{ color: 'var(--vscode-testing-iconPassed)' }}>
								<i className="codicon codicon-check"></i> SSH Agent Active
							</span>
						) : (
							<span style={{ color: 'var(--vscode-testing-iconFailed)' }}>
								<i className="codicon codicon-error"></i> Agent Not Found
							</span>
						)}
					</div>
				)}

				<div className="form-actions">
					<button type="button" onClick={onCancel} className="vscode-button secondary">Cancel</button>
					<button type="submit" className="vscode-button">Save</button>
				</div>
			</form>
		</div>
	);
};

export default EditHost;
