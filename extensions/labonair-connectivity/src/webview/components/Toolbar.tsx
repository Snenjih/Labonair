import React from 'react';

interface ToolbarProps {
	onRefresh: () => void;
	onImport: (format: 'json' | 'ssh-config') => void;
	onExport: () => void;
}

const Toolbar: React.FC<ToolbarProps> = ({ onRefresh, onImport, onExport }) => {

	const handleImportClick = () => {
		// For simplicity, just import SSH Config by default or show a small menu.
		// Instructions say "Import button... can import an SSH Config file".
		// But importer service supports JSON too.
		// Let's defaulted to ssh-config or maybe let user valid via separate buttons?
		// For now simple alert or default.
		// Actually, let's just trigger import ssh-config as primary action for now or open a choice logic if possible.
		// But easier is just to pass 'ssh-config' as it's the main requirement.
		onImport('ssh-config');
	};

	return (
		<div className="toolbar">
			<button onClick={onRefresh} title="Refresh">
				<i className="codicon codicon-refresh"></i>
			</button>
			<button onClick={handleImportClick} title="Import SSH Config">
				<i className="codicon codicon-cloud-upload"></i>
			</button>
			<button onClick={onExport} title="Export JSON">
				<i className="codicon codicon-cloud-download"></i>
			</button>
			<button title="Local Terminal">
				<i className="codicon codicon-terminal"></i>
			</button>
		</div>
	);
};

export default Toolbar;
