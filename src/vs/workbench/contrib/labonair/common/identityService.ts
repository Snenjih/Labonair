/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { createDecorator } from '../../../../platform/instantiation/common/instantiation.js';
import { Event } from '../../../../base/common/event.js';

export const IIdentityService = createDecorator<IIdentityService>('identityService');

export type IdentityType = 'ssh-key' | 'password';

export interface IIdentity {
	id: string;
	name: string;
	type: IdentityType;
	publicKey?: string;
	comment?: string;
	createdAt: number;
}

export interface IIdentityService {
	readonly _serviceBrand: undefined;

	readonly onDidChangeIdentities: Event<void>;

	getAllIdentities(): Promise<IIdentity[]>;
	getIdentity(id: string): Promise<IIdentity | undefined>;
	addIdentity(identity: IIdentity, privateData: string, passphrase?: string): Promise<void>;
	updateIdentity(id: string, identity: Partial<IIdentity>): Promise<void>;
	deleteIdentity(id: string): Promise<void>;

	getPrivateKey(id: string): Promise<string | undefined>;
	getPassword(id: string): Promise<string | undefined>;
}
