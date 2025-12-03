/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { EditorInput } from '../../../common/editor/editorInput.js';
import { URI } from '../../../../base/common/uri.js';
import { ThemeIcon } from '../../../../base/common/themables.js';
import { Codicon } from '../../../../base/common/codicons.js';

export class ThemeStudioInput extends EditorInput {
	static readonly ID = 'workbench.input.themeStudio';
	static readonly resource = URI.parse('themeStudio://default');

	private static _instance: ThemeStudioInput | undefined;

	static getInstance(): ThemeStudioInput {
		if (!ThemeStudioInput._instance) {
			ThemeStudioInput._instance = new ThemeStudioInput();
		}
		return ThemeStudioInput._instance;
	}

	constructor() {
		super();
	}

	override get typeId(): string {
		return ThemeStudioInput.ID;
	}

	override get resource(): URI {
		return ThemeStudioInput.resource;
	}

	override getName(): string {
		return 'Theme Studio';
	}

	override getIcon(): ThemeIcon {
		return Codicon.paintcan;
	}

	override matches(other: EditorInput | unknown): boolean {
		return other instanceof ThemeStudioInput;
	}
}
