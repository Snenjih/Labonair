/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { IBaseActionViewItemOptions } from '../../../../base/browser/ui/actionbar/actionViewItems.js';
import { IActivityHoverOptions } from '../compositeBarActions.js';
import { SimpleAccountActivityActionViewItem } from '../globalCompositeBar.js';
import { HoverPosition } from '../../../../base/browser/ui/hover/hoverWidget.js';
import { IThemeService } from '../../../../platform/theme/common/themeService.js';
import { ILifecycleService } from '../../../services/lifecycle/common/lifecycle.js';
import { IHoverService } from '../../../../platform/hover/browser/hover.js';
import { IContextMenuService } from '../../../../platform/contextview/browser/contextView.js';
import { IMenuService } from '../../../../platform/actions/common/actions.js';
import { IContextKeyService } from '../../../../platform/contextkey/common/contextkey.js';
import { IAuthenticationService } from '../../../services/authentication/common/authentication.js';
import { IWorkbenchEnvironmentService } from '../../../services/environment/common/environmentService.js';
import { IProductService } from '../../../../platform/product/common/productService.js';
import { IConfigurationService } from '../../../../platform/configuration/common/configuration.js';
import { IKeybindingService } from '../../../../platform/keybinding/common/keybinding.js';
import { ISecretStorageService } from '../../../../platform/secrets/common/secrets.js';
import { IStorageService } from '../../../../platform/storage/common/storage.js';
import { ILogService } from '../../../../platform/log/common/log.js';
import { IActivityService } from '../../../services/activity/common/activity.js';
import { IInstantiationService } from '../../../../platform/instantiation/common/instantiation.js';
import { ICommandService } from '../../../../platform/commands/common/commands.js';

/**
 * Titlebar Account Action View Item
 *
 * This component displays the account/user icon in the titlebar, optimized for the titlebar position.
 * It's based on SimpleAccountActivityActionViewItem but specifically designed for permanent titlebar placement.
 */
export class TitlebarAccountActionViewItem extends SimpleAccountActivityActionViewItem {

	constructor(
		options: IBaseActionViewItemOptions,
		@IThemeService themeService: IThemeService,
		@ILifecycleService lifecycleService: ILifecycleService,
		@IHoverService hoverService: IHoverService,
		@IContextMenuService contextMenuService: IContextMenuService,
		@IMenuService menuService: IMenuService,
		@IContextKeyService contextKeyService: IContextKeyService,
		@IAuthenticationService authenticationService: IAuthenticationService,
		@IWorkbenchEnvironmentService environmentService: IWorkbenchEnvironmentService,
		@IProductService productService: IProductService,
		@IConfigurationService configurationService: IConfigurationService,
		@IKeybindingService keybindingService: IKeybindingService,
		@ISecretStorageService secretStorageService: ISecretStorageService,
		@IStorageService storageService: IStorageService,
		@ILogService logService: ILogService,
		@IActivityService activityService: IActivityService,
		@IInstantiationService instantiationService: IInstantiationService,
		@ICommandService commandService: ICommandService
	) {
		const hoverOptions: IActivityHoverOptions = {
			position: () => HoverPosition.BELOW
		};

		super(
			hoverOptions,
			options,
			themeService,
			lifecycleService,
			hoverService,
			contextMenuService,
			menuService,
			contextKeyService,
			authenticationService,
			environmentService,
			productService,
			configurationService,
			keybindingService,
			secretStorageService,
			storageService,
			logService,
			activityService,
			instantiationService,
			commandService
		);
	}
}
