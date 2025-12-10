/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import * as dom from '../../../../base/browser/dom.js';
import { Disposable } from '../../../../base/common/lifecycle.js';
import { IQuickPickItem } from '../../../../platform/quickinput/common/quickInput.js';
import { ITextModelService } from '../../../../editor/common/services/resolverService.js';
import { URI } from '../../../../base/common/uri.js';
import { IKeybindingService } from '../../../../platform/keybinding/common/keybinding.js';

export interface IQuickInputPreviewItem {
	readonly resource?: URI;
	readonly command?: {
		id: string;
		title: string;
		category?: string;
	};
	readonly description?: string;
	readonly detail?: string;
}

export class QuickInputPreviewProvider extends Disposable {

	constructor(
		@ITextModelService private readonly textModelService: ITextModelService,
		@IKeybindingService private readonly keybindingService: IKeybindingService
	) {
		super();
	}

	async renderPreview(container: HTMLElement, item: IQuickPickItem | IQuickInputPreviewItem): Promise<void> {
		dom.clearNode(container);

		// Check if it's a file item (has resource)
		if ('resource' in item && item.resource) {
			await this.renderFilePreview(container, item.resource);
			return;
		}

		// Check if it's a command item
		if ('command' in item && item.command) {
			this.renderCommandPreview(container, item.command, item.description, item.detail);
			return;
		}

		// Fallback: render basic item info
		this.renderBasicPreview(container, item);
	}

	private async renderFilePreview(container: HTMLElement, resource: URI): Promise<void> {
		// Show loading state
		const loadingDiv = dom.append(container, dom.$('.quick-input-preview-loading'));
		const spinner = dom.append(loadingDiv, dom.$('.codicon.codicon-loading.codicon-modifier-spin'));
		spinner.style.fontSize = '16px';

		try {
			// Load file content
			const ref = await this.textModelService.createModelReference(resource);

			try {
				const model = ref.object.textEditorModel;
				const lineCount = model.getLineCount();
				const previewLines = Math.min(50, lineCount);

				// Clear loading state
				dom.clearNode(container);

				// Render header
				const header = dom.append(container, dom.$('.quick-input-preview-header'));
				header.textContent = resource.path.split('/').pop() || 'File Preview';

				// Render file info
				const infoSection = dom.append(container, dom.$('.quick-input-preview-section'));

				const pathLabel = dom.append(infoSection, dom.$('.quick-input-preview-label'));
				pathLabel.textContent = 'Path';

				const pathValue = dom.append(infoSection, dom.$('.quick-input-preview-value'));
				pathValue.textContent = resource.fsPath;
				pathValue.style.fontSize = '11px';
				pathValue.style.opacity = '0.8';
				pathValue.style.wordBreak = 'break-all';

				// Render content preview
				const contentSection = dom.append(container, dom.$('.quick-input-preview-section'));

				const contentLabel = dom.append(contentSection, dom.$('.quick-input-preview-label'));
				contentLabel.textContent = `Preview (${previewLines} of ${lineCount} lines)`;

				const codeBlock = dom.append(contentSection, dom.$('pre.quick-input-preview-value'));
				codeBlock.style.fontSize = '12px';
				codeBlock.style.fontFamily = 'var(--monaco-monospace-font)';
				codeBlock.style.whiteSpace = 'pre-wrap';
				codeBlock.style.wordBreak = 'break-word';
				codeBlock.style.backgroundColor = 'var(--vscode-editor-background)';
				codeBlock.style.padding = '8px';
				codeBlock.style.borderRadius = '4px';
				codeBlock.style.maxHeight = '300px';
				codeBlock.style.overflow = 'auto';

				// Get text content (first 50 lines)
				let content = '';
				for (let i = 1; i <= previewLines; i++) {
					content += model.getLineContent(i) + '\n';
				}

				codeBlock.textContent = content;

			} finally {
				ref.dispose();
			}

		} catch (error) {
			// Show error state
			dom.clearNode(container);
			const errorDiv = dom.append(container, dom.$('.quick-input-preview-section'));
			errorDiv.style.color = 'var(--vscode-errorForeground)';
			errorDiv.textContent = 'Failed to load file preview';
		}
	}

	private renderCommandPreview(container: HTMLElement, command: { id: string; title: string; category?: string }, description?: string, detail?: string): void {
		// Render header
		const header = dom.append(container, dom.$('.quick-input-preview-header'));
		header.textContent = command.title;

		// Render category
		if (command.category) {
			const categorySection = dom.append(container, dom.$('.quick-input-preview-section'));
			const categoryLabel = dom.append(categorySection, dom.$('.quick-input-preview-label'));
			categoryLabel.textContent = 'Category';
			const categoryValue = dom.append(categorySection, dom.$('.quick-input-preview-value'));
			categoryValue.textContent = command.category;
		}

		// Render command ID
		const idSection = dom.append(container, dom.$('.quick-input-preview-section'));
		const idLabel = dom.append(idSection, dom.$('.quick-input-preview-label'));
		idLabel.textContent = 'Command ID';
		const idValue = dom.append(idSection, dom.$('.quick-input-preview-value'));
		idValue.textContent = command.id;
		idValue.style.fontSize = '11px';
		idValue.style.fontFamily = 'var(--monaco-monospace-font)';
		idValue.style.opacity = '0.8';

		// Render keybinding
		const keybindings = this.keybindingService.lookupKeybindings(command.id);
		if (keybindings.length > 0) {
			const firstKeybinding = keybindings[0];
			if (firstKeybinding) {
				const kbSection = dom.append(container, dom.$('.quick-input-preview-section'));
				const kbLabel = dom.append(kbSection, dom.$('.quick-input-preview-label'));
				kbLabel.textContent = 'Keybinding';
				const kbValue = dom.append(kbSection, dom.$('.quick-input-preview-value'));
				kbValue.textContent = firstKeybinding.getLabel() || '';
				kbValue.style.fontWeight = '600';
			}
		}

		// Render description
		if (description) {
			const descSection = dom.append(container, dom.$('.quick-input-preview-section'));
			const descLabel = dom.append(descSection, dom.$('.quick-input-preview-label'));
			descLabel.textContent = 'Description';
			const descValue = dom.append(descSection, dom.$('.quick-input-preview-value'));
			descValue.textContent = description;
		}

		// Render detail
		if (detail) {
			const detailSection = dom.append(container, dom.$('.quick-input-preview-section'));
			const detailLabel = dom.append(detailSection, dom.$('.quick-input-preview-label'));
			detailLabel.textContent = 'Details';
			const detailValue = dom.append(detailSection, dom.$('.quick-input-preview-value'));
			detailValue.textContent = detail;
			detailValue.style.opacity = '0.8';
		}
	}

	private renderBasicPreview(container: HTMLElement, item: IQuickPickItem | IQuickInputPreviewItem): void {
		// Render label
		if ('label' in item && item.label) {
			const header = dom.append(container, dom.$('.quick-input-preview-header'));
			const label: any = item.label;
			header.textContent = typeof label === 'string' ? label : (label?.label || '');
		}

		// Render description
		if ('description' in item && item.description) {
			const descSection = dom.append(container, dom.$('.quick-input-preview-section'));
			const descLabel = dom.append(descSection, dom.$('.quick-input-preview-label'));
			descLabel.textContent = 'Description';
			const descValue = dom.append(descSection, dom.$('.quick-input-preview-value'));
			descValue.textContent = item.description;
		}

		// Render detail
		if ('detail' in item && item.detail) {
			const detailSection = dom.append(container, dom.$('.quick-input-preview-section'));
			const detailLabel = dom.append(detailSection, dom.$('.quick-input-preview-label'));
			detailLabel.textContent = 'Details';
			const detailValue = dom.append(detailSection, dom.$('.quick-input-preview-value'));
			detailValue.textContent = item.detail;
		}

		// If no content rendered, show a message
		if (container.children.length === 0) {
			const emptyDiv = dom.append(container, dom.$('.quick-input-preview-section'));
			emptyDiv.style.color = 'var(--vscode-descriptionForeground)';
			emptyDiv.style.fontStyle = 'italic';
			emptyDiv.textContent = 'No preview available';
		}
	}

	showLoading(container: HTMLElement): void {
		dom.clearNode(container);
		const loadingDiv = dom.append(container, dom.$('.quick-input-preview-loading'));
		const spinner = dom.append(loadingDiv, dom.$('.codicon.codicon-loading.codicon-modifier-spin'));
		spinner.style.fontSize = '16px';
	}
}
