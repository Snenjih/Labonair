/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

import * as dom from '../../../base/browser/dom.js';
import * as domStylesheetsJs from '../../../base/browser/domStylesheets.js';
import * as cssJs from '../../../base/browser/cssValue.js';
import { DomEmitter } from '../../../base/browser/event.js';
import { Event } from '../../../base/common/event.js';
import { StandardKeyboardEvent } from '../../../base/browser/keyboardEvent.js';
import { Gesture, EventType as GestureEventType } from '../../../base/browser/touch.js';
import { renderLabelWithIcons } from '../../../base/browser/ui/iconLabel/iconLabels.js';
import { IdGenerator } from '../../../base/common/idGenerator.js';
import { KeyCode } from '../../../base/common/keyCodes.js';
import { parseLinkedText } from '../../../base/common/linkedText.js';
import { URI } from '../../../base/common/uri.js';
import './media/quickInput.css';
import { localize } from '../../../nls.js';
import { DisposableStore } from '../../../base/common/lifecycle.js';
import { IQuickInputButton } from '../common/quickInput.js';
import { IAction } from '../../../base/common/actions.js';

const iconPathToClass: Record<string, string> = {};
const iconClassGenerator = new IdGenerator('quick-input-button-icon-');

function getIconClass(iconPath: { dark: URI; light?: URI } | undefined): string | undefined {
	if (!iconPath) {
		return undefined;
	}
	let iconClass: string;

	const key = iconPath.dark.toString();
	if (iconPathToClass[key]) {
		iconClass = iconPathToClass[key];
	} else {
		iconClass = iconClassGenerator.nextId();
		domStylesheetsJs.createCSSRule(`.${iconClass}, .hc-light .${iconClass}`, `background-image: ${cssJs.asCSSUrl(iconPath.light || iconPath.dark)}`);
		domStylesheetsJs.createCSSRule(`.vs-dark .${iconClass}, .hc-black .${iconClass}`, `background-image: ${cssJs.asCSSUrl(iconPath.dark)}`);
		iconPathToClass[key] = iconClass;
	}

	return iconClass;
}

export function quickInputButtonToAction(button: IQuickInputButton, id: string, run: () => unknown): IAction {
	let cssClasses = button.iconClass || getIconClass(button.iconPath);
	if (button.alwaysVisible) {
		cssClasses = cssClasses ? `${cssClasses} always-visible` : 'always-visible';
	}

	return {
		id,
		label: '',
		tooltip: button.tooltip || '',
		class: cssClasses,
		enabled: true,
		run
	};
}

export function renderQuickInputDescription(description: string, container: HTMLElement, actionHandler: { callback: (content: string) => void; disposables: DisposableStore }) {
	dom.reset(container);
	const parsed = parseLinkedText(description);
	let tabIndex = 0;
	for (const node of parsed.nodes) {
		if (typeof node === 'string') {
			container.append(...renderLabelWithIcons(node));
		} else {
			let title = node.title;

			if (!title && node.href.startsWith('command:')) {
				title = localize('executeCommand', "Click to execute command '{0}'", node.href.substring('command:'.length));
			} else if (!title) {
				title = node.href;
			}

			const anchor = dom.$('a', { href: node.href, title, tabIndex: tabIndex++ }, node.label);
			anchor.style.textDecoration = 'underline';
			const handleOpen = (e: unknown) => {
				if (dom.isEventLike(e)) {
					dom.EventHelper.stop(e, true);
				}

				actionHandler.callback(node.href);
			};

			const onClick = actionHandler.disposables.add(new DomEmitter(anchor, dom.EventType.CLICK)).event;
			const onKeydown = actionHandler.disposables.add(new DomEmitter(anchor, dom.EventType.KEY_DOWN)).event;
			const onSpaceOrEnter = Event.chain(onKeydown, $ => $.filter(e => {
				const event = new StandardKeyboardEvent(e);

				return event.equals(KeyCode.Space) || event.equals(KeyCode.Enter);
			}));

			actionHandler.disposables.add(Gesture.addTarget(anchor));
			const onTap = actionHandler.disposables.add(new DomEmitter(anchor, GestureEventType.Tap)).event;

			Event.any(onClick, onTap, onSpaceOrEnter)(handleOpen, null, actionHandler.disposables);
			container.appendChild(anchor);
		}
	}
}

/**
 * Safe mathematical expression evaluator for calculator mode
 * Supports basic operators: +, -, *, /, ^, %
 * Does not use eval() for security
 */
export function evaluateMathExpression(expression: string): number | null {
	try {
		// Remove whitespace
		let expr = expression.trim();

		// Check if expression starts with '=' and remove it
		if (expr.startsWith('=')) {
			expr = expr.substring(1).trim();
		}

		// Check for valid characters (numbers, operators, parentheses, decimal points)
		if (!/^[\d+\-*/%^().\s]+$/.test(expr)) {
			return null;
		}

		// Replace ^ with ** for exponentiation
		expr = expr.replace(/\^/g, '**');

		// Parse and evaluate the expression using a safe parser
		const result = parseExpression(expr);

		if (result === null || !isFinite(result) || isNaN(result)) {
			return null;
		}

		return result;
	} catch {
		return null;
	}
}

/**
 * Check if input looks like a mathematical expression
 */
export function isMathExpression(input: string): boolean {
	const trimmed = input.trim();

	// Starts with '='
	if (trimmed.startsWith('=')) {
		return true;
	}

	// Contains math operators and numbers (but not just a filename pattern)
	// Must have at least one operator and one number
	const hasMathOperator = /[+\-*/%^]/.test(trimmed);
	const hasNumber = /\d/.test(trimmed);
	const hasValidPattern = /^\d+\s*[+\-*/%^]\s*\d+/.test(trimmed);

	return hasMathOperator && hasNumber && hasValidPattern;
}

/**
 * Safe expression parser using recursive descent
 */
function parseExpression(expr: string): number | null {
	try {
		let pos = 0;

		function parseNumber(): number | null {
			const match = expr.substring(pos).match(/^-?\d+(\.\d+)?/);
			if (!match) {
				return null;
			}
			pos += match[0].length;
			return parseFloat(match[0]);
		}

		function skipWhitespace(): void {
			while (pos < expr.length && /\s/.test(expr[pos])) {
				pos++;
			}
		}

		function parseFactor(): number | null {
			skipWhitespace();

			// Handle parentheses
			if (expr[pos] === '(') {
				pos++;
				const result = parseAddSub();
				if (result === null) {
					return null;
				}
				skipWhitespace();
				if (expr[pos] !== ')') {
					return null;
				}
				pos++;
				return result;
			}

			// Handle numbers
			return parseNumber();
		}

		function parsePower(): number | null {
			let left = parseFactor();
			if (left === null) {
				return null;
			}

			skipWhitespace();
			while (pos < expr.length && expr.substring(pos, pos + 2) === '**') {
				pos += 2;
				const right = parseFactor();
				if (right === null) {
					return null;
				}
				left = Math.pow(left, right);
				skipWhitespace();
			}

			return left;
		}

		function parseMulDiv(): number | null {
			let left = parsePower();
			if (left === null) {
				return null;
			}

			skipWhitespace();
			while (pos < expr.length && (expr[pos] === '*' || expr[pos] === '/' || expr[pos] === '%')) {
				const op = expr[pos];
				pos++;
				const right = parsePower();
				if (right === null) {
					return null;
				}

				if (op === '*') {
					left = left * right;
				} else if (op === '/') {
					if (right === 0) {
						return null; // Division by zero
					}
					left = left / right;
				} else if (op === '%') {
					if (right === 0) {
						return null; // Modulo by zero
					}
					left = left % right;
				}
				skipWhitespace();
			}

			return left;
		}

		function parseAddSub(): number | null {
			let left = parseMulDiv();
			if (left === null) {
				return null;
			}

			skipWhitespace();
			while (pos < expr.length && (expr[pos] === '+' || expr[pos] === '-')) {
				const op = expr[pos];
				pos++;
				const right = parseMulDiv();
				if (right === null) {
					return null;
				}

				if (op === '+') {
					left = left + right;
				} else if (op === '-') {
					left = left - right;
				}
				skipWhitespace();
			}

			return left;
		}

		const result = parseAddSub();
		skipWhitespace();

		// Check if we consumed the entire expression
		if (pos !== expr.length) {
			return null;
		}

		return result;
	} catch {
		return null;
	}
}
