/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import { EditorPane } from '../../../browser/parts/editor/editorPane.js';
import { ITelemetryService } from '../../../../platform/telemetry/common/telemetry.js';
import { IThemeService } from '../../../../platform/theme/common/themeService.js';
import { IStorageService } from '../../../../platform/storage/common/storage.js';
import { ThemeStudioInput } from './themeStudioInput.js';
import { CancellationToken } from '../../../../base/common/cancellation.js';
import { Dimension } from '../../../../base/browser/dom.js';
import { IUserThemeService, IUserTheme } from '../../../services/themes/common/userThemeService.js';
import { DisposableStore } from '../../../../base/common/lifecycle.js';
import { IFileDialogService } from '../../../../platform/dialogs/common/dialogs.js';
import { IFileService } from '../../../../platform/files/common/files.js';
import { VSBuffer } from '../../../../base/common/buffer.js';
import { IWorkbenchThemeService } from '../../../services/themes/common/workbenchThemeService.js';
import { INotificationService } from '../../../../platform/notification/common/notification.js';

export class ThemeStudioEditor extends EditorPane {
	static readonly ID = 'workbench.editor.themeStudio';

	private container: HTMLElement | undefined;
	private disposables = this._register(new DisposableStore());

	constructor(
		group: any,
		@ITelemetryService telemetryService: ITelemetryService,
		@IThemeService themeService: IThemeService,
		@IStorageService storageService: IStorageService,
		@IUserThemeService private readonly userThemeService: IUserThemeService,
		@IFileDialogService private readonly fileDialogService: IFileDialogService,
		@IFileService private readonly fileService: IFileService,
		@IWorkbenchThemeService private readonly workbenchThemeService: IWorkbenchThemeService,
		@INotificationService private readonly notificationService: INotificationService
	) {
		super(ThemeStudioEditor.ID, group, telemetryService, themeService, storageService);
	}

	override async setInput(input: ThemeStudioInput, options: any, context: any, token: CancellationToken): Promise<void> {
		await super.setInput(input, options, context, token);

		if (this.container) {
			this.render();
		}
	}

	protected override createEditor(parent: HTMLElement): void {
		this.container = parent;
		this.container.classList.add('theme-studio-editor');
		this.render();
	}

	private async render(): Promise<void> {
		if (!this.container) {
			return;
		}

		this.container.innerHTML = '';

		// Create main structure
		const wrapper = document.createElement('div');
		wrapper.className = 'theme-studio';

		// Header
		const header = this.createHeader();
		wrapper.appendChild(header);

		// Main body with theme grid
		const main = document.createElement('main');
		main.className = 'theme-studio-body';

		const themeGrid = document.createElement('div');
		themeGrid.className = 'theme-grid';
		themeGrid.id = 'theme-grid';

		// Load and render themes
		await this.renderThemes(themeGrid);

		main.appendChild(themeGrid);
		wrapper.appendChild(main);

		// Editor panel (initially hidden)
		const editorPanel = document.createElement('aside');
		editorPanel.className = 'theme-editor-panel hidden';
		editorPanel.id = 'editor-panel';
		wrapper.appendChild(editorPanel);

		this.container.appendChild(wrapper);
		this.attachEventListeners(wrapper);
	}

	private createHeader(): HTMLElement {
		const header = document.createElement('header');
		header.className = 'theme-studio-header';

		// Tabs section
		const tabs = document.createElement('div');
		tabs.className = 'theme-studio-tabs';

		const tabsData = [
			{ id: 'colors', label: 'Color Themes', active: true },
			{ id: 'file-icons', label: 'File Icon Themes', active: false },
			{ id: 'product-icons', label: 'Product Icon Themes', active: false }
		];

		tabsData.forEach(tab => {
			const button = document.createElement('button');
			button.className = `tab ${tab.active ? 'active' : ''}`;
			button.dataset.tab = tab.id;
			button.textContent = tab.label;
			tabs.appendChild(button);
		});

		header.appendChild(tabs);

		// Actions section
		const actions = document.createElement('div');
		actions.className = 'theme-studio-actions';

		const search = document.createElement('input');
		search.type = 'search';
		search.placeholder = 'Search themes...';
		search.className = 'theme-search';

		const createBtn = document.createElement('button');
		createBtn.className = 'btn-primary';
		createBtn.id = 'create-new';
		createBtn.textContent = 'Create New';

		const importBtn = document.createElement('button');
		importBtn.className = 'btn-secondary';
		importBtn.id = 'import-theme';
		importBtn.textContent = 'Import';

		actions.appendChild(search);
		actions.appendChild(createBtn);
		actions.appendChild(importBtn);

		header.appendChild(actions);

		return header;
	}

	private async renderThemes(container: HTMLElement): Promise<void> {
		const themes = await this.userThemeService.getUserThemes();

		if (themes.length === 0) {
			const emptyState = document.createElement('div');
			emptyState.className = 'empty-state';
			emptyState.innerHTML = `
				<div class="empty-state-icon">🎨</div>
				<h2>No Custom Themes Yet</h2>
				<p>Create your first custom theme to get started</p>
			`;
			container.appendChild(emptyState);
			return;
		}

		themes.forEach(theme => {
			const card = this.createThemeCard(theme);
			container.appendChild(card);
		});
	}

	private createThemeCard(theme: IUserTheme): HTMLElement {
		const card = document.createElement('div');
		card.className = 'theme-card';
		card.dataset.themeId = theme.id;

		// Preview
		const preview = document.createElement('div');
		preview.className = 'theme-preview';
		preview.style.backgroundColor = theme.colors['editor.background'] || '#1e1e1e';
		card.appendChild(preview);

		// Info
		const info = document.createElement('div');
		info.className = 'theme-info';

		const name = document.createElement('h3');
		name.textContent = theme.name;

		const meta = document.createElement('div');
		meta.className = 'theme-meta';
		meta.textContent = `${theme.type.toUpperCase()} • ${new Date(theme.metadata.createdAt).toLocaleDateString()}`;

		info.appendChild(name);
		info.appendChild(meta);
		card.appendChild(info);

		// Actions
		const actionsDiv = document.createElement('div');
		actionsDiv.className = 'theme-actions';

		const editBtn = document.createElement('button');
		editBtn.className = 'action-btn';
		editBtn.textContent = 'Edit';
		editBtn.onclick = () => this.editTheme(theme.id);

		const exportBtn = document.createElement('button');
		exportBtn.className = 'action-btn';
		exportBtn.textContent = 'Export';
		exportBtn.onclick = () => this.exportTheme(theme.id);

		const deleteBtn = document.createElement('button');
		deleteBtn.className = 'action-btn danger';
		deleteBtn.textContent = 'Delete';
		deleteBtn.onclick = () => this.deleteTheme(theme.id);

		actionsDiv.appendChild(editBtn);
		actionsDiv.appendChild(exportBtn);
		actionsDiv.appendChild(deleteBtn);

		card.appendChild(actionsDiv);

		return card;
	}

	private attachEventListeners(wrapper: HTMLElement): void {
		// Create new button
		const createBtn = wrapper.querySelector('#create-new');
		if (createBtn) {
			createBtn.addEventListener('click', () => this.createNewTheme());
		}

		// Import button
		const importBtn = wrapper.querySelector('#import-theme');
		if (importBtn) {
			importBtn.addEventListener('click', () => this.importTheme());
		}

		// Search
		const search = wrapper.querySelector('.theme-search') as HTMLInputElement;
		if (search) {
			search.addEventListener('input', (e) => this.filterThemes((e.target as HTMLInputElement).value));
		}
	}

	private async createNewTheme(): Promise<void> {
		const theme: IUserTheme = {
			id: '',
			name: 'New Theme',
			type: 'dark',
			colors: {
				'editor.background': '#1e1e1e',
				'editor.foreground': '#d4d4d4'
			},
			tokenColors: [],
			metadata: {
				createdAt: Date.now(),
				updatedAt: Date.now()
			}
		};

		const created = await this.userThemeService.createUserTheme(theme);
		this.editTheme(created.id);
	}

	private async editTheme(themeId: string): Promise<void> {
		const theme = await this.userThemeService.getUserTheme(themeId);
		if (!theme) {
			return;
		}

		const editorPanel = this.container?.querySelector('#editor-panel');
		if (!editorPanel) {
			return;
		}

		editorPanel.innerHTML = '';
		editorPanel.classList.remove('hidden');

		// Header
		const header = document.createElement('div');
		header.className = 'editor-panel-header';

		const title = document.createElement('h2');
		title.textContent = 'Edit Theme';

		const closeBtn = document.createElement('button');
		closeBtn.className = 'close-btn';
		closeBtn.textContent = '×';
		closeBtn.onclick = () => editorPanel.classList.add('hidden');

		header.appendChild(title);
		header.appendChild(closeBtn);
		editorPanel.appendChild(header);

		// Theme name input
		const nameSection = document.createElement('div');
		nameSection.className = 'editor-section';

		const nameLabel = document.createElement('label');
		nameLabel.textContent = 'Theme Name';

		const nameInput = document.createElement('input');
		nameInput.type = 'text';
		nameInput.value = theme.name;
		nameInput.className = 'theme-name-input';

		nameSection.appendChild(nameLabel);
		nameSection.appendChild(nameInput);
		editorPanel.appendChild(nameSection);

		// Theme type selection
		const typeSection = document.createElement('div');
		typeSection.className = 'editor-section';

		const typeLabel = document.createElement('label');
		typeLabel.textContent = 'Theme Type';

		const typeSelect = document.createElement('select');
		typeSelect.className = 'theme-type-select';
		['dark', 'light', 'hc'].forEach(type => {
			const option = document.createElement('option');
			option.value = type;
			option.textContent = type.toUpperCase();
			option.selected = theme.type === type;
			typeSelect.appendChild(option);
		});

		typeSection.appendChild(typeLabel);
		typeSection.appendChild(typeSelect);
		editorPanel.appendChild(typeSection);

		// Base theme selection
		const baseThemeSection = document.createElement('div');
		baseThemeSection.className = 'editor-section';

		const baseThemeLabel = document.createElement('label');
		baseThemeLabel.textContent = 'Base Theme (Inherit From)';

		const baseThemeSelect = document.createElement('select');
		baseThemeSelect.className = 'base-theme-select';

		// Add "None" option
		const noneOption = document.createElement('option');
		noneOption.value = '';
		noneOption.textContent = 'None (Start from scratch)';
		noneOption.selected = !theme.baseTheme;
		baseThemeSelect.appendChild(noneOption);

		// Load and add available themes
		this.workbenchThemeService.getColorThemes().then(themes => {
			themes.forEach(availableTheme => {
				const option = document.createElement('option');
				option.value = availableTheme.id;
				option.textContent = availableTheme.label;
				option.selected = theme.baseTheme === availableTheme.id;
				baseThemeSelect.appendChild(option);
			});
		});

		baseThemeSection.appendChild(baseThemeLabel);
		baseThemeSection.appendChild(baseThemeSelect);
		editorPanel.appendChild(baseThemeSection);

		// Color groups
		const colorGroups = this.getColorGroups();
		for (const [groupName, colorKeys] of Object.entries(colorGroups)) {
			const groupSection = this.createColorGroupSection(groupName, colorKeys, theme);
			editorPanel.appendChild(groupSection);
		}

		// Action buttons
		const actions = document.createElement('div');
		actions.className = 'editor-panel-actions';

		const saveBtn = document.createElement('button');
		saveBtn.className = 'btn-primary';
		saveBtn.textContent = 'Save';
		saveBtn.onclick = async () => {
			await this.saveTheme(themeId, {
				name: nameInput.value,
				type: typeSelect.value as any,
				baseTheme: baseThemeSelect.value || undefined
			});
			editorPanel.classList.add('hidden');
			this.render();
		};

		const cancelBtn = document.createElement('button');
		cancelBtn.className = 'btn-secondary';
		cancelBtn.textContent = 'Cancel';
		cancelBtn.onclick = () => editorPanel.classList.add('hidden');

		actions.appendChild(saveBtn);
		actions.appendChild(cancelBtn);
		editorPanel.appendChild(actions);
	}

	private createColorGroupSection(groupName: string, colorKeys: string[], theme: IUserTheme): HTMLElement {
		const section = document.createElement('div');
		section.className = 'color-group-section';

		const header = document.createElement('h3');
		header.className = 'color-group-header';
		header.textContent = groupName;
		section.appendChild(header);

		const colorGrid = document.createElement('div');
		colorGrid.className = 'color-grid';

		colorKeys.forEach(colorKey => {
			const colorItem = document.createElement('div');
			colorItem.className = 'color-item';

			const label = document.createElement('label');
			label.textContent = this.formatColorLabel(colorKey);
			label.className = 'color-label';

			const inputWrapper = document.createElement('div');
			inputWrapper.className = 'color-input-wrapper';

			const colorInput = document.createElement('input');
			colorInput.type = 'color';
			colorInput.value = theme.colors[colorKey] || '#000000';
			colorInput.dataset.colorKey = colorKey;
			colorInput.className = 'color-picker';

			const hexInput = document.createElement('input');
			hexInput.type = 'text';
			hexInput.value = theme.colors[colorKey] || '#000000';
			hexInput.className = 'hex-input';
			hexInput.maxLength = 7;

			colorInput.oninput = () => {
				hexInput.value = colorInput.value;
				theme.colors[colorKey] = colorInput.value;
			};

			hexInput.oninput = () => {
				if (hexInput.value.match(/^#[0-9A-Fa-f]{6}$/)) {
					colorInput.value = hexInput.value;
					theme.colors[colorKey] = hexInput.value;
				}
			};

			inputWrapper.appendChild(colorInput);
			inputWrapper.appendChild(hexInput);

			colorItem.appendChild(label);
			colorItem.appendChild(inputWrapper);
			colorGrid.appendChild(colorItem);
		});

		section.appendChild(colorGrid);
		return section;
	}

	private getColorGroups(): Record<string, string[]> {
		return {
			'Editor Colors': [
				'editor.background',
				'editor.foreground',
				'editor.lineHighlightBackground',
				'editor.selectionBackground',
				'editorCursor.foreground',
				'editorLineNumber.foreground'
			],
			'UI Colors': [
				'activityBar.background',
				'activityBar.foreground',
				'sideBar.background',
				'sideBar.foreground',
				'statusBar.background',
				'statusBar.foreground'
			],
			'Syntax Colors': [
				'editorBracketMatch.background',
				'editorBracketMatch.border',
				'editorError.foreground',
				'editorWarning.foreground',
				'editorInfo.foreground'
			]
		};
	}

	private formatColorLabel(colorKey: string): string {
		return colorKey
			.split('.')
			.map(part => part.charAt(0).toUpperCase() + part.slice(1))
			.join(' ');
	}

	private async saveTheme(themeId: string, updates: Partial<IUserTheme>): Promise<void> {
		try {
			await this.userThemeService.updateUserTheme(themeId, updates);
		} catch (error) {
			console.error('Failed to save theme:', error);
		}
	}

	private async exportTheme(themeId: string): Promise<void> {
		try {
			const theme = await this.userThemeService.getUserTheme(themeId);
			if (!theme) {
				return;
			}

			const json = await this.userThemeService.exportTheme(themeId);

			// Show save dialog
			const defaultUri = await this.fileDialogService.defaultFilePath();
			const result = await this.fileDialogService.showSaveDialog({
				title: 'Export Theme',
				defaultUri,
				filters: [{ name: 'JSON', extensions: ['json'] }]
			});

			if (result) {
				await this.fileService.writeFile(result, VSBuffer.fromString(json));
				this.notificationService.info(`Theme "${theme.name}" exported successfully to ${result.fsPath}`);
			}
		} catch (error) {
			this.notificationService.error(`Failed to export theme: ${error}`);
		}
	}

	private async deleteTheme(themeId: string): Promise<void> {
		if (confirm('Are you sure you want to delete this theme?')) {
			try {
				await this.userThemeService.deleteUserTheme(themeId);
				this.notificationService.info('Theme deleted successfully');
				this.render(); // Refresh the view
			} catch (error) {
				this.notificationService.error(`Failed to delete theme: ${error}`);
			}
		}
	}

	private async importTheme(): Promise<void> {
		try {
			// Show open dialog
			const result = await this.fileDialogService.showOpenDialog({
				title: 'Import Theme',
				canSelectFiles: true,
				canSelectMany: false,
				filters: [{ name: 'JSON', extensions: ['json'] }]
			});

			if (result && result.length > 0) {
				const fileContent = await this.fileService.readFile(result[0]);
				const json = fileContent.value.toString();
				const importedTheme = await this.userThemeService.importTheme(json);
				this.notificationService.info(`Theme "${importedTheme.name}" imported successfully`);
				this.render(); // Refresh the view
			}
		} catch (error) {
			this.notificationService.error('Failed to import theme. Please check the file format and try again.');
		}
	}

	private filterThemes(query: string): void {
		const cards = this.container?.querySelectorAll('.theme-card');
		if (!cards) {
			return;
		}

		const lowerQuery = query.toLowerCase();
		cards.forEach(card => {
			const name = card.querySelector('h3')?.textContent?.toLowerCase() || '';
			if (name.includes(lowerQuery)) {
				(card as HTMLElement).style.display = '';
			} else {
				(card as HTMLElement).style.display = 'none';
			}
		});
	}

	override layout(dimension: Dimension): void {
		// Handle layout changes
	}

	override clearInput(): void {
		super.clearInput();
		this.disposables.clear();
	}

	override dispose(): void {
		this.disposables.dispose();
		super.dispose();
	}
}
