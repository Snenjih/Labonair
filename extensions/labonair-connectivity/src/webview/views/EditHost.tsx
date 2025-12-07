import React, { useState } from 'react';
import { Host } from '../../common/types';

interface EditHostProps {
	initialHost?: Host | null;
	onSave: (host: Host, password?: string, keyPath?: string) => void;
	onCancel: () => void;
}

const EditHost: React.FC<EditHostProps> = ({ initialHost, onSave, onCancel }) => {
	const [activeTab, setActiveTab] = useState<'general' | 'auth'>('general');

	// Form State
	const [name, setName] = useState(initialHost?.name || '');
	const [group, setGroup] = useState(initialHost?.group || '');
	const [host, setHost] = useState(initialHost?.host || '');
	const [port, setPort] = useState(initialHost?.port?.toString() || '22');
	const [username, setUsername] = useState(initialHost?.username || '');
	const [osIcon, setOsIcon] = useState<Host['osIcon']>(initialHost?.osIcon || 'linux');
	const [tags, setTags] = useState(initialHost?.tags?.join(', ') || '');

	// Auth State (Not persisted in Host object directly)
	const [password, setPassword] = useState('');
	const [keyPath, setKeyPath] = useState('');

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
			tags: tags.split(',').map(t => t.trim()).filter(Boolean)
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
					SSH / Auth
				</button>
			</div>

			<form onSubmit={handleSubmit}>
				{activeTab === 'general' && (
					<div className="form-group">
						<label>Label</label>
						<input value={name} onChange={e => setName(e.target.value)} required />

						<label>Group</label>
						<input value={group} onChange={e => setGroup(e.target.value)} />

						<label>Tags (comma separated)</label>
						<input value={tags} onChange={e => setTags(e.target.value)} />

						<label>OS Icon</label>
						<select value={osIcon} onChange={e => setOsIcon(e.target.value as any)}>
							<option value="linux">Linux</option>
							<option value="windows">Windows</option>
							<option value="mac">macOS</option>
							<option value="other">Other</option>
						</select>
					</div>
				)}

				{activeTab === 'auth' && (
					<div className="form-group">
						<label>Host Address</label>
						<input value={host} onChange={e => setHost(e.target.value)} required />

						<label>Port</label>
						<input type="number" value={port} onChange={e => setPort(e.target.value)} />

						<label>Username</label>
						<input value={username} onChange={e => setUsername(e.target.value)} required />

						<label>Password</label>
						<input type="password" value={password} onChange={e => setPassword(e.target.value)} placeholder="Leave empty to keep unchanged" />

						<label>Private Key Path</label>
						<input value={keyPath} onChange={e => setKeyPath(e.target.value)} placeholder="~/.ssh/id_rsa" />
					</div>
				)}

				<div className="form-actions">
					<button type="button" onClick={onCancel} className="btn-secondary">Cancel</button>
					<button type="submit" className="btn-primary">Save</button>
				</div>
			</form>
		</div>
	);
};

export default EditHost;
