/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import * as vscode from 'vscode';
import { HostService } from './hostService';
import { IdentityService } from './identityService';
import { ScriptService } from './scriptService';
import { HostViewProvider } from './hostViewProvider';
import { IdentityViewProvider } from './identityViewProvider';
import { ScriptViewProvider } from './scriptViewProvider';

let hostService: HostService;
let identityService: IdentityService;
let scriptService: ScriptService;

export function activate(context: vscode.ExtensionContext) {
	console.log('[Labonair] Activating extension...');

	// Initialize services
	hostService = new HostService(context);
	identityService = new IdentityService(context);
	scriptService = new ScriptService(context);

	// Register view providers
	const hostViewProvider = new HostViewProvider(context, hostService);
	const identityViewProvider = new IdentityViewProvider(context, identityService);
	const scriptViewProvider = new ScriptViewProvider(context, scriptService);

	context.subscriptions.push(
		vscode.window.registerWebviewViewProvider('labonair.views.hosts', hostViewProvider),
		vscode.window.registerWebviewViewProvider('labonair.views.identities', identityViewProvider),
		vscode.window.registerWebviewViewProvider('labonair.views.scripts', scriptViewProvider)
	);

	// Register commands
	context.subscriptions.push(
		vscode.commands.registerCommand('labonair.addHost', async () => {
			vscode.window.showInformationMessage('Add Host command triggered');
			// Command will be handled by the view provider
		}),
		vscode.commands.registerCommand('labonair.connectSSH', async () => {
			vscode.window.showInformationMessage('Connect SSH command triggered');
		}),
		vscode.commands.registerCommand('labonair.refreshHosts', async () => {
			hostViewProvider.refresh();
		}),
		vscode.commands.registerCommand('labonair.addIdentity', async () => {
			vscode.window.showInformationMessage('Add Identity command triggered');
		}),
		vscode.commands.registerCommand('labonair.refreshIdentities', async () => {
			identityViewProvider.refresh();
		}),
		vscode.commands.registerCommand('labonair.addScript', async () => {
			vscode.window.showInformationMessage('Add Script command triggered');
		}),
		vscode.commands.registerCommand('labonair.refreshScripts', async () => {
			scriptViewProvider.refresh();
		})
	);

	console.log('[Labonair] Extension activated successfully');
}

export function deactivate() {
	console.log('[Labonair] Deactivating extension...');

	// Dispose services
	if (hostService) {
		hostService.dispose();
	}
	if (identityService) {
		identityService.dispose();
	}
	if (scriptService) {
		scriptService.dispose();
	}

	console.log('[Labonair] Extension deactivated');
}
