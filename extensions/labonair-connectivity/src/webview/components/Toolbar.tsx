import React from 'react';

interface ToolbarProps {
	onRefresh: () => void;
	onImport: (format: 'json' | 'ssh-config') => void;
	onExport: () => void;
	onSort: (criteria: 'name' | 'lastUsed' | 'group') => void;
	sortCriteria?: 'name' | 'lastUsed' | 'group';
}

const Toolbar: React.FC<ToolbarProps> = ({ onRefresh, onImport, onExport, onSort, sortCriteria }) => {

	const handleImportClick = () => {
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

			<div className="toolbar-separator"></div>

			<div className="dropdown-wrapper">
				<i className="codicon codicon-sort-precedence dropdown-icon"></i>
				<select className="toolbar-select" onChange={(e) => onSort(e.target.value as any)} defaultValue="name">
					<option value="name">Name</option>
					<option value="lastUsed">Last Used</option>
					<option value="group">Group</option>
				</select>
			</div>

			<div className="toolbar-separator"></div>

			<button title="Local Terminal">
				<i className="codicon codicon-terminal"></i>
			</button>
		</div>
	);
};

export default Toolbar;
