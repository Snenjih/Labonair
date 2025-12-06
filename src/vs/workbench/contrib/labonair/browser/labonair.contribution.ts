/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import './media/hostManager.css';
import { localize, localize2 } from '../../../../nls.js';
import { Registry } from '../../../../platform/registry/common/platform.js';
import { SyncDescriptor } from '../../../../platform/instantiation/common/descriptors.js';
import { registerSingleton, InstantiationType } from '../../../../platform/instantiation/common/extensions.js';
import { IViewContainersRegistry, ViewContainerLocation, Extensions as ViewContainerExtensions, IViewsRegistry } from '../../../common/views.js';
import { LabonairHostView } from './hostView.js';
import { VIEWLET_ID, VIEW_PANE_ID } from '../common/labonair.js';
import { Codicon } from '../../../../base/common/codicons.js';
import { registerIcon } from '../../../../platform/theme/common/iconRegistry.js';
import { IHostService } from '../common/hostService.js';
import { HostService } from './hostService.js';
import { IIdentityService } from '../common/identityService.js';
import { IdentityService } from './identityService.js';
import { ISSHConfigService } from '../common/sshConfigService.js';
import { SSHConfigService } from './sshConfigService.js';
import { KeybindingsRegistry, KeybindingWeight } from '../../../../platform/keybinding/common/keybindingsRegistry.js';
import { KeyMod, KeyCode } from '../../../../base/common/keyCodes.js';
import { ICommandService } from '../../../../platform/commands/common/commands.js';
import { ServicesAccessor } from '../../../../platform/instantiation/common/instantiation.js';
import { IQuickAccessRegistry, Extensions as QuickAccessExtensions } from '../../../../platform/quickinput/common/quickAccess.js';
import { HostQuickAccessProvider } from './hostQuickAccess.js';

const labonairViewIcon = registerIcon('labonair-view-icon', Codicon.serverProcess, localize('labonairViewIcon', 'View icon of the Labonair Host Manager view.'));

const viewContainer = Registry.as<IViewContainersRegistry>(ViewContainerExtensions.ViewContainersRegistry).registerViewContainer({
	id: VIEWLET_ID,
	title: localize2('labonair', 'Labonair'),
	ctorDescriptor: new SyncDescriptor(
		class LabonairViewPaneContainer {
			constructor() { }
		}
	),
	storageId: 'workbench.labonair.views.state',
	icon: labonairViewIcon,
	alwaysUseContainerInfo: true,
	order: 10,
	hideIfEmpty: false,
}, ViewContainerLocation.Sidebar, { doNotRegisterOpenCommand: false });

const viewsRegistry = Registry.as<IViewsRegistry>(ViewContainerExtensions.ViewsRegistry);

viewsRegistry.registerViews([{
	id: VIEW_PANE_ID,
	name: localize2('hosts', "Hosts"),
	containerIcon: labonairViewIcon,
	ctorDescriptor: new SyncDescriptor(LabonairHostView),
	canToggleVisibility: true,
	canMoveView: true,
	hideByDefault: false,
	collapsed: false,
	order: 1,
	weight: 100,
	focusCommand: { id: 'workbench.view.labonair.focus' }
}], viewContainer);

registerSingleton(IHostService, HostService, InstantiationType.Delayed);
registerSingleton(IIdentityService, IdentityService, InstantiationType.Delayed);
registerSingleton(ISSHConfigService, SSHConfigService, InstantiationType.Delayed);

KeybindingsRegistry.registerCommandAndKeybindingRule({
	id: 'workbench.view.labonair.focus',
	weight: KeybindingWeight.WorkbenchContrib,
	when: undefined,
	primary: KeyMod.Alt | KeyCode.KeyH,
	handler: (accessor: ServicesAccessor) => {
		const commandService = accessor.get(ICommandService);
		return commandService.executeCommand('workbench.view.extension.labonair');
	}
});

const quickAccessRegistry = Registry.as<IQuickAccessRegistry>(QuickAccessExtensions.Quickaccess);
quickAccessRegistry.registerQuickAccessProvider({
	ctor: HostQuickAccessProvider,
	prefix: HostQuickAccessProvider.PREFIX,
	placeholder: localize('hostQuickAccessPlaceholder', "Type the name of a host to connect"),
	helpEntries: [
		{
			description: localize('hostQuickAccess', "Show Labonair Hosts"),
			prefix: HostQuickAccessProvider.PREFIX,
			commandId: 'workbench.view.labonair.focus'
		}
	]
});
