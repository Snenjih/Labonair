import React, { useState } from 'react';

interface HostGroupProps {
	name: string;
	count: number;
	children: React.ReactNode;
}

const HostGroup: React.FC<HostGroupProps> = ({ name, count, children }) => {
	const [expanded, setExpanded] = useState(true);

	return (
		<div className="host-group">
			<div className="group-header" onClick={() => setExpanded(!expanded)}>
				<i className={`codicon codicon-chevron-${expanded ? 'down' : 'right'}`}></i>
				<span className="group-icon">
					<i className="codicon codicon-folder"></i>
				</span>
				<span className="group-name">{name}</span>
				<span className="group-count">({count})</span>
			</div>
			{expanded && <div className="group-content">{children}</div>}
		</div>
	);
};

export default HostGroup;
