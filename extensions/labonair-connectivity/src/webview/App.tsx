import React, { useEffect, useState } from 'react';
import { vscodeApi } from './utils/vscode';
import { Host } from '../common/types';

export const App: React.FC = () => {
	const [hosts, setHosts] = useState<Host[]>([]);

	useEffect(() => {
		// Request initial data
		vscodeApi.postMessage({ command: 'GET_HOSTS' });

		// Listen for messages from extension
		const handleMessage = (event: MessageEvent) => {
			const message = event.data;
			switch (message.command) {
				case 'UPDATE_HOSTS':
					setHosts(message.payload);
					break;
			}
		};

		window.addEventListener('message', handleMessage);
		return () => window.removeEventListener('message', handleMessage);
	}, []);

	return (
		<div style={{ padding: '10px' }}>
			<h1>Host Manager</h1>
			<p>Welcome to Labonair Connectivity</p>
			<ul>
				{hosts.map(host => (
					<li key={host.id}>{host.name} ({host.host})</li>
				))}
			</ul>
		</div>
	);
};
