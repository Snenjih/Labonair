import React from 'react';
import { Host } from '../../common/types';

interface HostCardProps {
	host: Host;
	onConnect: () => void;
	onEdit: () => void;
	onDelete: () => void;
}

const HostCard: React.FC<HostCardProps> = ({ host, onConnect, onEdit, onDelete }) => {
	return (
		<div className="host-card">
			<div className="card-top">
				<input type="checkbox" />
				<div className="host-info">
					<div className="host-name">
						<i className={`codicon codicon-${host.osIcon === 'windows' ? 'window' : 'terminal-linux'}`}></i>
						{host.name}
					</div>
					<div className="host-address">
						{host.username}@{host.host}:{host.port}
					</div>
				</div>
			</div>

			<div className="card-middle">
				{host.tags.map((tag, index) => (
					<span key={index} className="tag-pill">{tag}</span>
				))}
			</div>

			<div className="card-bottom">
				<button className="action-btn" title="Stats">
					<i className="codicon codicon-graph"></i>
				</button>
				<button className="action-btn" title="SSH" onClick={onConnect}>
					<i className="codicon codicon-remote"></i>
				</button>
				<button className="action-btn" title="SFTP">
					<i className="codicon codicon-file-symlink-directory"></i>
				</button>
				<button className="action-btn secondary" onClick={onEdit} title="Edit">
					<i className="codicon codicon-edit"></i>
				</button>
				<button className="action-btn secondary" onClick={onDelete} title="Delete">
					<i className="codicon codicon-trash"></i>
				</button>
			</div>
		</div>
	);
};

export default HostCard;
