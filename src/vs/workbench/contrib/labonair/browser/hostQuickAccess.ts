/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { localize } from '../../../../nls.js';
import { IQuickPickSeparator } from '../../../../platform/quickinput/common/quickInput.js';
import { IPickerQuickAccessItem, PickerQuickAccessProvider, TriggerAction } from '../../../../platform/quickinput/browser/pickerQuickAccess.js';
import { matchesFuzzy } from '../../../../base/common/filters.js';
import { IHostService, IHost } from '../common/hostService.js';
import { ICommandService } from '../../../../platform/commands/common/commands.js';
import { ThemeIcon } from '../../../../base/common/themables.js';
import { Codicon } from '../../../../base/common/codicons.js';

export class HostQuickAccessProvider extends PickerQuickAccessProvider<IPickerQuickAccessItem> {
	static PREFIX = 'host ';

	constructor(
		@IHostService private readonly hostService: IHostService,
		@ICommandService private readonly commandService: ICommandService
	) {
		super(HostQuickAccessProvider.PREFIX, { canAcceptInBackground: true });
	}

	protected async _getPicks(filter: string): Promise<Array<IPickerQuickAccessItem | IQuickPickSeparator>> {
		const picks: Array<IPickerQuickAccessItem | IQuickPickSeparator> = [];
		const hosts = await this.hostService.getAllHosts();

		const groupedHosts = new Map<string, IHost[]>();
		for (const host of hosts) {
			const group = host.group || 'Ungrouped';
			if (!groupedHosts.has(group)) {
				groupedHosts.set(group, []);
			}
			groupedHosts.get(group)!.push(host);
		}

		for (const [groupName, groupHosts] of groupedHosts) {
			if (groupHosts.length === 0) {
				continue;
			}

			picks.push({ type: 'separator', label: groupName });

			for (const host of groupHosts) {
				const label = `${host.name} (${host.connection.username}@${host.connection.host})`;
				const highlights = matchesFuzzy(filter, label);

				if (!highlights && filter) {
					continue;
				}

				const status = this.hostService.getHostStatus(host.id);
				const statusIcon = this._getStatusIcon(status);

				picks.push({
					label: `$(${statusIcon.id}) ${label}`,
					ariaLabel: label,
					description: host.tags?.join(', '),
					highlights: highlights ? { label: highlights } : undefined,
					buttons: [
						{
							iconClass: ThemeIcon.asClassName(Codicon.terminal),
							tooltip: localize('connectSSH', "Connect via SSH")
						},
						{
							iconClass: ThemeIcon.asClassName(Codicon.folder),
							tooltip: localize('connectSFTP', "Open SFTP")
						}
					],
					accept: () => this._connectToHost(host, 'ssh'),
					trigger: (buttonIndex) => {
						if (buttonIndex === 0) {
							this._connectToHost(host, 'ssh');
							return TriggerAction.CLOSE_PICKER;
						} else if (buttonIndex === 1) {
							this._connectToHost(host, 'sftp');
							return TriggerAction.CLOSE_PICKER;
						}
						return TriggerAction.NO_ACTION;
					}
				});
			}
		}

		if (picks.length === 0) {
			picks.push({
				label: localize('noHosts', "No hosts configured"),
				accept: () => { }
			});
		}

		picks.push({ type: 'separator' });
		picks.push({
			label: `$(plus) ${localize('addHost', "Add New Host")}`,
			ariaLabel: localize('addHost', "Add New Host"),
			accept: () => this.commandService.executeCommand('labonair.addHost')
		});

		return picks;
	}

	private _getStatusIcon(status: string): ThemeIcon {
		switch (status) {
			case 'online':
				return Codicon.circleFilled;
			case 'offline':
				return Codicon.circleOutline;
			case 'connecting':
				return Codicon.loading;
			default:
				return Codicon.circle;
		}
	}

	private _connectToHost(host: IHost, type: 'ssh' | 'sftp'): boolean {
		if (type === 'ssh') {
			this.commandService.executeCommand('labonair.connectSSH', host.id);
		} else {
			this.commandService.executeCommand('labonair.connectSFTP', host.id);
		}
		return true;
	}
}
