import React from 'react';

interface ToolbarProps {
	onRefresh: () => void;
}

const Toolbar: React.FC<ToolbarProps> = ({ onRefresh }) => {
	return (
		<div className="toolbar">
			<button onClick={onRefresh} title="Refresh">
				<i className="codicon codicon-refresh"></i>
			</button>
			<button title="Import JSON">
				<i className="codicon codicon-cloud-upload"></i>
			</button>
			<button title="Local Terminal">
				<i className="codicon codicon-terminal"></i>
			</button>
		</div>
	);
};

export default Toolbar;
