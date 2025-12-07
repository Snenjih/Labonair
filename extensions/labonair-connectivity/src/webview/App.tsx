import React, { useEffect, useState } from 'react';
import vscode from './utils/vscode';
import { Host, WebviewState, Message } from '../common/types';
import TopNav from './components/TopNav';
import Toolbar from './components/Toolbar';
import HostGroup from './components/HostGroup';
import HostCard from './components/HostCard';
import EditHost from './views/EditHost';

const App: React.FC = () => {
	const [state, setState] = useState<WebviewState>({
		view: 'list',
		hosts: [],
		selectedHost: null
	});

	useEffect(() => {
		window.addEventListener('message', event => {
			const message: Message = event.data;
			switch (message.command) {
				case 'UPDATE_DATA':
					setState(prev => ({ ...prev, hosts: message.payload.hosts }));
					break;
			}
		});

		// Initial fetch
		vscode.postMessage({ command: 'FETCH_DATA' });
	}, []);

	const handleNavigate = (view: 'list' | 'edit' | 'credentials') => {
		setState(prev => ({ ...prev, view }));
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

	// Group hosts by group name
	const groupedHosts = state.hosts.reduce((acc, host) => {
		const group = host.group || 'Ungrouped';
		if (!acc[group]) acc[group] = [];
		acc[group].push(host);
		return acc;
	}, {} as Record<string, Host[]>);

	return (
		<div className="app-container">
			<TopNav activeView={state.view} onNavigate={handleNavigate} />

			{state.view === 'list' && (
				<>
					<Toolbar
						onRefresh={() => vscode.postMessage({ command: 'FETCH_DATA' })}
						onImport={handleImport}
						onExport={handleExport}
					/>
					<div className="host-list">
						{Object.entries(groupedHosts).map(([group, hosts]) => (
							<HostGroup key={group} name={group} count={hosts.length}>
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
				</>
			)}

			{state.view === 'edit' && (
				<EditHost
					initialHost={state.selectedHost}
					onSave={handleSaveHost}
					onCancel={() => setState(prev => ({ ...prev, view: 'list', selectedHost: null }))}
				/>
			)}

			{state.view === 'credentials' && (
				<div className="placeholder-view">Credentials Manager (Coming Soon)</div>
			)}
		</div>
	);
};

export default App;
