/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { localize } from '../../../../nls.js';
import { registerEditorPane, EditorPaneDescriptor } from '../../../browser/editor.js';
import { EditorExtensions, IEditorFactoryRegistry } from '../../../common/editor.js';
import { ThemeStudioEditor } from './themeStudioEditor.js';
import { ThemeStudioInput } from './themeStudioInput.js';
import { Registry } from '../../../../platform/registry/common/platform.js';
import { SyncDescriptor } from '../../../../platform/instantiation/common/descriptors.js';
import { registerAction2, Action2 } from '../../../../platform/actions/common/actions.js';
import { IEditorService } from '../../../services/editor/common/editorService.js';
import { ServicesAccessor } from '../../../../platform/instantiation/common/instantiation.js';
import { Categories } from '../../../../platform/action/common/actionCommonCategories.js';
import { EditorInput } from '../../../common/editor/editorInput.js';

// Register the editor pane
registerEditorPane(
	EditorPaneDescriptor.create(
		ThemeStudioEditor,
		ThemeStudioEditor.ID,
		localize('themeStudio', "Theme Studio")
	),
	[
		new SyncDescriptor(ThemeStudioInput)
	]
);

// Register editor input factory
class ThemeStudioInputFactory implements any {
	canSerialize(): boolean {
		return true;
	}

	serialize(editorInput: EditorInput): string {
		return '';
	}

	deserialize(instantiationService: any, serializedEditorInput: string): EditorInput {
		return ThemeStudioInput.getInstance();
	}
}

Registry.as<IEditorFactoryRegistry>(EditorExtensions.EditorFactory).registerEditorFactory(
	ThemeStudioInput.ID,
	ThemeStudioInputFactory
);

// Register command to open Theme Studio
registerAction2(class OpenThemeStudioAction extends Action2 {
	constructor() {
		super({
			id: 'workbench.action.openThemeStudio',
			title: { value: localize('openThemeStudio', "Open Theme Studio"), original: 'Open Theme Studio' },
			category: Categories.Preferences,
			f1: true
		});
	}

	async run(accessor: ServicesAccessor): Promise<void> {
		const editorService = accessor.get(IEditorService);
		await editorService.openEditor(ThemeStudioInput.getInstance(), { pinned: true });
	}
});
