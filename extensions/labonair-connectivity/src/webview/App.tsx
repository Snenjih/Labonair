import React, { useEffect, useState } from 'react';
import vscode from './utils/vscode';
import { Host, WebviewState, Message, Credential } from '../common/types';
import TopNav from './components/TopNav';
import Toolbar from './components/Toolbar';
import HostGroup from './components/HostGroup';
import HostCard from './components/HostCard';
import EditHost from './views/EditHost';
import CredentialsView from './views/CredentialsView';
import ScriptList from './components/ScriptList';
import SearchBar from './components/SearchBar';

const App: React.FC = () => {
	const [state, setState] = useState<WebviewState>({
		view: 'list',
		hosts: [],
		selectedHost: null,
		credentials: [],
		scripts: [],
		activeSessionHostIds: []
	});

	const [filterText, setFilterText] = useState('');
	const [sortCriteria, setSortCriteria] = useState<'name' | 'lastUsed' | 'group'>('name');

	useEffect(() => {
		window.addEventListener('message', event => {
			const message: Message = event.data;
			switch (message.command) {
				case 'UPDATE_DATA':
					setState(prev => ({
						...prev,
						hosts: message.payload.hosts || prev.hosts,
						credentials: message.payload.credentials || prev.credentials,
						scripts: message.payload.scripts || prev.scripts,
						activeSessionHostIds: message.payload.activeSessionHostIds !== undefined ? message.payload.activeSessionHostIds : prev.activeSessionHostIds
					}));
					break;
				case 'SESSION_UPDATE':
					setState(prev => ({
						...prev,
						activeSessionHostIds: message.payload.activeHostIds
					}));
					break;
			}
		});

		// Initial fetch
		vscode.postMessage({ command: 'FETCH_DATA' });
	}, []);

	const handleNavigate = (view: 'list' | 'edit' | 'credentials') => {
		setState(prev => ({ ...prev, view }));
		if (view === 'credentials' && (!state.credentials || state.credentials.length === 0)) {
			vscode.postMessage({ command: 'GET_CREDENTIALS' });
		}
	};

	const handleSaveHost = (host: Host, password?: string, keyPath?: string) => {
		vscode.postMessage({ command: 'SAVE_HOST', payload: { host, password, keyPath } });
		setState(prev => ({ ...prev, view: 'list' }));
	};

	const handleDeleteHost = (id: string) => {
		vscode.postMessage({ command: 'DELETE_HOST', payload: { id } });
	};

	const handleConnect = (id: string) => {
		vscode.postMessage({ command: 'CONNECT_SSH', payload: { id } });
	};

	const handleImport = (format: 'json' | 'ssh-config') => {
		vscode.postMessage({ command: 'IMPORT_REQUEST', payload: { format } });
	};

	const handleExport = () => {
		vscode.postMessage({ command: 'EXPORT_REQUEST' });
	};

	const handleRefresh = () => { // Added handleRefresh
		vscode.postMessage({ command: 'FETCH_DATA' });
	};

	// Filtering
	const filteredHosts = state.hosts.filter(h => {
		if (!filterText) return true;
		const lower = filterText.toLowerCase();
		return h.name.toLowerCase().includes(lower) ||
			h.host.toLowerCase().includes(lower) ||
			h.tags.some(t => t.toLowerCase().includes(lower));
	});

	// Sorting
	const sortedHosts = [...filteredHosts].sort((a, b) => {
		if (sortCriteria === 'name') return a.name.localeCompare(b.name);
		if (sortCriteria === 'lastUsed') return (b.lastUsed || 0) - (a.lastUsed || 0);
		if (sortCriteria === 'group') return (a.group || 'Ungrouped').localeCompare(b.group || 'Ungrouped');
		return 0;
	});

	// Grouping
	const groupedHosts: Record<string, Host[]> = {};
	sortedHosts.forEach(host => {
		const group = host.group || 'Ungrouped';
		if (!groupedHosts[group]) groupedHosts[group] = [];
		groupedHosts[group].push(host);
	});

	return (
		<div className="app-container">
			<TopNav activeView={state.view} onNavigate={handleNavigate} />

			{state.view === 'list' && (
				<>
					<Toolbar
						onRefresh={handleRefresh} // Changed to handleRefresh
						onImport={handleImport}
						onExport={handleExport}
						onSort={setSortCriteria} // Added onSort prop
						sortCriteria={sortCriteria} // Added sortCriteria prop
					/>
					<SearchBar value={filterText} onChange={setFilterText} /> {/* Added SearchBar */}
					<div className="host-list">
						{Object.entries(groupedHosts).map(([group, hosts]) => (
							<HostGroup key={group} name={group} count={hosts.length} credentials={state.credentials}>
								{hosts.map(host => (
									<HostCard
										key={host.id}
										host={host}
										onConnect={() => handleConnect(host.id)}
										onDelete={() => handleDeleteHost(host.id)}
										onEdit={() => {
											setState(prev => ({ ...prev, view: 'edit', selectedHost: host }));
										}}
									/>
								))}
							</HostGroup>
						))}
					</div>
					<ScriptList scripts={state.scripts || []} />
				</>
			)}

			{state.view === 'edit' && (
				<EditHost
					initialHost={state.selectedHost}
					agentAvailable={state.sshAgentAvailable}
					onSave={handleSaveHost}
					onCancel={() => setState(prev => ({ ...prev, view: 'list', selectedHost: null }))}
				/>
			)}

			{state.view === 'credentials' && (
				<CredentialsView credentials={state.credentials || []} />
			)}
		</div>
	);
};

export default App;
