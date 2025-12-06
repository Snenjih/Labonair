# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Projektübersicht

**Projektname**: Labonair
**Basis**: Visual Studio Code Fork (Open Source)
**Projektziel**: Ein datenschutzorientierter, schlanker und benutzerfreundlicher Code-Editor mit revolutionärem UI/UX-Design, der sich von der Microsoft-Telemetrie und Cloud-Abhängigkeiten befreit und gleichzeitig innovative Features für Theme-Management, Settings-Verwaltung und Benutzerinteraktion einführt.

**Architekturübersicht**: Labonair basiert auf der VS Code Architektur - einem mehrschichtigen, serviceorientierten System mit strikten Abhängigkeitsgrenzen:
- **Base Layer** (`src/vs/base/`): Plattformübergreifende Utilities
- **Platform Layer** (`src/vs/platform/`): 95+ injectable Services
- **Workbench Layer** (`src/vs/workbench/`): Anwendungs-UI und Komposition
- **Editor Layer** (`src/vs/editor/`): Monaco Editor Integration
- **Contributions** (`src/vs/workbench/contrib/`): Feature-Beiträge (91+ Features)

**Technologie-Stack**:
- **Core**: TypeScript, Electron
- **Build System**: Gulp, npm
- **Testing**: Mocha, Playwright (Smoke Tests)
- **UI**: Custom DOM-basierte UI-Komponenten
- **Dienste**: Dependency Injection mit Decorators
- **Architektur**: Multi-Process (Main, Renderer, Extension Host, Shared Process)

---

## Operational & Maintenance Protocols

### 1. Performance & Resource Constraints
Labonair adds heavy features (SSH listeners, Pings, Webviews) to a web-based app. To prevent "bloat":
*   **Lazy Loading:** ALL views (Host Manager, Theme Studio) must only load their heavy data when they are *visible*.
*   **Ping Intervals:** Network pings in the Host Manager must run in a non-blocking way (async) and not more often than every 60s.
*   **Webview Lifecycle:** Webviews consume high RAM.
    *   Use `retainContextWhenHidden: true` ONLY for active operations (SSH connection, File Transfer).
    *   Static views (Theme Studio) should dispose/serialize state when hidden if memory pressure is high.
*   **Disposable Pattern:** Every listener (`.onDid...`) must be pushed to a `subscriptions` array and disposed when the view closes. Memory leaks in a long-running IDE are unacceptable.

### 2. Production Build Pipeline
The command `npm run watch` is for development only. It is slow and unoptimized.
To create the shippable product:
*   **Clean Build:** Always run `git clean -fdX` before a release build to remove stale artifacts.
*   **Minification:** Use the specific platform gulp tasks:
    *   macOS (Apple Silicon): `npm run gulp vscode-darwin-arm64-min`
    *   Windows: `npm run gulp vscode-win32-x64-min`
    *   Linux: `npm run gulp vscode-linux-x64-min`
*   **Signing:** (Future Scope) Builds must be signed to run on macOS Gatekeeper/Windows SmartScreen without warnings.

### 3. Troubleshooting Common Fork Issues
*   **"Corrupted Installation":** If VS Code complains about corruption, it's often due to modified core checksums.
    *   *Fix:* Disable checksum verification in `product.json` during dev (`checksums: { ... }`).
*   **Native Module Mismatch:** If `ssh2` or `node-pty` crashes:
    *   *Fix:* Run `npm run electron-rebuild` to compile native C++ modules against the specific Electron version used by VS Code.

---

## Implementation Guidelines & Architectural Standards

**CRITICAL:** All development within the Labonair project must adhere strictly to the following architectural patterns and standards to ensure maintainability, security, and seamless integration with the existing VS Code core.

### 1. Directory Structure & Module Location
*   **New Features:** All new core features (Host Manager, Theme Studio, Dashboard) must be implemented as **Workbench Contributions**.
    *   Path: `src/vs/workbench/contrib/[feature-name]/`
*   **Shared Services:** Services required by multiple contributions (e.g., `UserThemeService`) reside in:
    *   Path: `src/vs/workbench/services/[service-name]/`
*   **Built-in Extensions:** Standalone tools (Hex Editor, RegEx Studio) that do not require deep core access go to:
    *   Path: `extensions/labonair-[feature-name]/`

### 2. UI/UX Design System
Labonair aims for a "Native & Industrial Clean" look. To guarantee theme compatibility:
*   **No Hardcoded Colors:** NEVER use hex codes (e.g., `#ffffff`) in CSS.
*   **CSS Variables:** ALWAYS use VS Code native CSS variables.
    *   *Backgrounds:* `var(--vscode-editor-background)`, `var(--vscode-sideBar-background)`
    *   *Text:* `var(--vscode-foreground)`, `var(--vscode-descriptionForeground)`
    *   *Borders:* `var(--vscode-panel-border)`, `var(--vscode-focusBorder)`
    *   *Interactive:* `var(--vscode-list-hoverBackground)`, `var(--vscode-button-background)`
*   **Icons:** Use the native Codicon library (`<i class="codicon codicon-remote"></i>`) for UI consistency.

### 3. Data Persistence & Security
*   **Configuration Files:** User-generated content (Hosts, Themes) must be stored in the user's data directory, NOT in the workspace.
    *   Path: `~/.labonair/` (Platform dependent user data path).
    *   Format: JSON.
*   **Secrets & Credentials:** Passwords, SSH Keys, and Tokens must **NEVER** be written to JSON files or logs.
    *   **Requirement:** Use the `ISecretStorageService` (VS Code Core API) to store sensitive data in the OS Keychain/Credential Manager.
*   **State:** Use `IStorageService` for non-critical UI state (e.g., "Last active tab", "Panel size").

### 4. Webview Architecture
Complex interfaces (Host Manager, Theme Studio) use Webviews. The architecture must follow the **Message Passing Pattern**:
*   **Frontend (Webview):** Dumb rendering layer (HTML/CSS/JS). No direct FS access. Sends events via `vscode.postMessage({ command: 'saveHost', payload: ... })`.
*   **Backend (Editor/ViewPane):** Handles business logic, filesystem operations, and connection handling. Listens via `webview.onDidReceiveMessage`.
*   **State Sync:** The Backend pushes state updates to the Frontend. The Frontend does not hold state that isn't persisted in the Backend.

### 5. Coding Standards
*   **Dependency Injection:** Use the VS Code Instantiation Service. Do not create service instances manually via `new`. Use `@IServiceName` decorators in constructors.
*   **Disposables:** All event listeners and resources must be registered to a `DisposableStore` or extend `Disposable` to prevent memory leaks.
*   **Promises:** Use `async/await` over raw Promises where possible.

### 6. Testing & Debugging
*   **Dev Build:** Always run `npm run watch` in one terminal and `./scripts/code.sh` (or `.bat`) in another.
*   **Webview Debugging:** Open the Developer Tools (`Help > Toggle Developer Tools`) and switch to the context of the Webview (iframe) to debug UI logic.
*   **Reloading:** Use `Reload Window` (Cmd+R) to apply changes to the Renderer process (UI/Webviews). Changes to the Main process require a full restart.


## Project Overview

This is **Labonair**, the open-source codebase for Visual Studio Code. It's a large-scale TypeScript application built on Electron, using a layered service-oriented architecture with strict dependency boundaries.

## Build and Development Commands

### Initial Setup
```bash
npm i                           # Install dependencies (run after clone)
```

### Running VS Code
```bash
./scripts/code.sh               # Launch development build (macOS/Linux)
.\scripts\code.bat              # Launch development build (Windows)
./scripts/code-web.sh           # Launch web version
./scripts/code-server.sh        # Launch remote server
```

### Building and Compiling
```bash
npm run compile                 # Full compilation (client + extensions)
npm run watch                   # Watch mode (client + extensions)
npm run watch-client            # Watch client files only
npm run watch-extensions        # Watch extensions only
npm run compile-web             # Build web version
npm run compile-cli             # Compile CLI tool (Rust)
npm run compile-build           # Production build with mangling
```

### Testing

**IMPORTANT**: Always check for TypeScript compilation errors before running tests. Never run tests if there are compilation errors.

```bash
# Unit tests
./scripts/test.sh               # Electron unit tests
./scripts/test.sh --grep <pattern>  # Run specific tests matching pattern
npm run test-node               # Node.js unit tests
npm run test-browser            # Browser unit tests

# Integration tests
./scripts/test-integration.sh   # All integration tests
./scripts/test-integration.sh --grep <pattern>  # Specific integration tests
./scripts/test-web-integration.sh  # Web integration tests
./scripts/test-remote-integration.sh  # Remote integration tests

# End-to-end tests
npm run smoketest               # Full smoke tests (requires build)
npm run smoketest-no-compile    # Run smoke tests without compiling

# Other checks
npm run eslint                  # Run linter
npm run stylelint               # Run CSS linter
npm run valid-layers-check      # Validate architectural layers
npm run monaco-compile-check    # Check Monaco editor types
npm run vscode-dts-compile-check  # Check VS Code API types
```

### Development Utilities
```bash
npm run gulp <task>             # Run specific gulp task
npm run hygiene                 # Run code hygiene checks
npm run update-grammars         # Update TextMate grammars
npm run download-builtin-extensions  # Download built-in extensions
```

## High-Level Architecture

### Layered Architecture

The codebase follows a strict layered architecture enforced by TypeScript configuration:

```
src/vs/
├── base/              # Foundation layer - utilities and cross-platform abstractions
├── platform/          # Platform services layer - 95+ injectable services
├── editor/            # Monaco editor integration
├── workbench/         # Application UI and composition
├── code/              # Application entry points (electron-main, cli)
└── server/            # Remote server implementation
```

**Layer Rules:**
- Lower layers cannot import from higher layers
- Browser code cannot import Node.js code
- Worker code has restricted imports
- Violations break the build (`npm run valid-layers-check`)

### Directory Structure

- **`src/vs/base/`** - Foundation utilities organized by platform:
  - `common/` - Platform-agnostic utilities (async, events, lifecycle, strings, URI)
  - `browser/` - Browser-specific code (DOM, CSS, workers)
  - `node/` - Node.js-specific code (file system, processes)

- **`src/vs/platform/`** - Cross-cutting services (95+ services):
  - Services are defined as interfaces (e.g., `IFileService`)
  - Implemented per-platform (`browser/`, `node/`, `electron-browser/`)
  - Injected via dependency injection using decorators
  - Examples: files, configuration, keybinding, dialogs, telemetry, editor, extensions

- **`src/vs/editor/`** - Monaco editor integration:
  - `common/` - Editor interfaces and types
  - `browser/` - Browser editor implementation
  - `contrib/` - Editor contributions (features)
  - `standalone/` - Standalone Monaco build

- **`src/vs/workbench/`** - VS Code application:
  - `api/` - Extension host and VS Code API implementation
  - `browser/` - Workbench UI (parts, views, actions, layout)
  - `services/` - Workbench-specific services
  - `contrib/` - Feature contributions (91+ features like git, debug, search, terminal)
  - `common/` - Shared workbench interfaces

- **`extensions/`** - Built-in extensions (60+):
  - Language support (`*-language-features/`)
  - Core features (git, debug, emmet, markdown)
  - Themes (`theme-*`)
  - Each follows standard extension structure with `package.json`

- **`cli/`** - VS Code CLI (Rust implementation)

- **`build/`** - Build scripts and gulp tasks

- **`test/`** - Integration tests and test infrastructure

- **`scripts/`** - Development and utility scripts

### Key Architectural Patterns

#### 1. Service-Oriented Architecture (Dependency Injection)

Services are the building blocks of VS Code. Define and use services like this:

```typescript
// Define service interface with decorator
export const IMyService = createDecorator<IMyService>('myService');

export interface IMyService {
  readonly _serviceBrand: undefined;
  doSomething(): void;
}

// Implement service
class MyService implements IMyService {
  declare readonly _serviceBrand: undefined;

  // Inject dependencies via constructor
  constructor(
    @IFileService private readonly fileService: IFileService,
    @ILogService private readonly logService: ILogService
  ) {}

  doSomething(): void {
    // Implementation
  }
}

// Register service in ServiceCollection
serviceCollection.set(IMyService, new SyncDescriptor(MyService));
```

Services are injected automatically via constructor parameters decorated with `@ServiceName`.

#### 2. Contribution Model

Features register themselves via contribution points:
- Commands (`CommandsRegistry`)
- Menus (`MenuRegistry`)
- Configuration (`ConfigurationRegistry`)
- Views, actions, keybindings, etc.

Contributions are collected at startup and composed into the workbench.

#### 3. Disposable Pattern

All resources implement `IDisposable`:
- Always dispose resources when done
- Use `DisposableStore` for hierarchical disposal
- Prevents memory leaks

```typescript
const disposables = new DisposableStore();
disposables.add(event.onDidChange(() => {}));
// Later:
disposables.dispose();  // Cleans up all registered disposables
```

#### 4. Event-Driven Communication

Use typed events for decoupled communication:

```typescript
private readonly _onDidChange = new Emitter<string>();
readonly onDidChange: Event<string> = this._onDidChange.event;

// Fire event
this._onDidChange.fire('value');

// Listen to event
const disposable = service.onDidChange(value => {
  // Handle change
});
```

#### 5. Multi-Process Architecture (Electron)

VS Code runs across multiple processes:
- **Main process** - Window management, native APIs
- **Renderer process(es)** - UI and editor
- **Extension host** - Extension execution (isolated)
- **Shared process** - Singleton services

Communication via typed IPC channels with proxy generation.

### Important Subsystems

- **Language Server Protocol** - `src/vs/workbench/services/language*` - Extensible language support
- **Debug Adapter Protocol** - `src/vs/workbench/contrib/debug/` - Universal debugging interface
- **Terminal** - `src/vs/platform/terminal/` and `src/vs/workbench/contrib/terminal/`
- **File System** - `src/vs/platform/files/` - URI-based abstraction supporting local, remote, virtual
- **Source Control** - `src/vs/workbench/contrib/scm/` - Git integration and SCM providers
- **Extension API** - `src/vscode-dts/` - Public VS Code API (`vscode.d.ts` for stable, `vscode.proposed.*.d.ts` for experimental)

## Coding Guidelines

### Indentation and Formatting

- **Use tabs, not spaces** for indentation
- Open curly braces on same line
- Always use curly braces for loops and conditionals
- No surrounding whitespace in parenthesized constructs

### Naming Conventions

- `PascalCase` for types, enums, and enum values
- `camelCase` for functions, methods, properties, and local variables
- Service interfaces prefixed with `I` (e.g., `IFileService`)
- Use whole words when possible

### TypeScript

- Do not export types or functions unless needed across components
- Do not introduce global namespace types or values
- Do not use `any` or `unknown` unless absolutely necessary - define proper types
- Never duplicate imports - reuse existing imports

### Strings

- Use double quotes `"` for user-facing strings that need localization
- Use single quotes `'` for all other strings
- Externalize user-facing strings with `vs/nls` module
- Use placeholders `{0}`, `{1}` instead of string concatenation for localized strings

### Functions

- Prefer arrow functions `=>` over anonymous functions
- Only surround arrow parameters when necessary:
  - `x => x + x` ✓
  - `(x) => x + x` ✗
  - `(x, y) => x + y` ✓
  - `<T>(x: T) => x` ✓
- Use `export function` instead of `export const` in top-level scopes for better stack traces
- Prefer `async`/`await` over `.then()` chains

### Comments and Documentation

- Use JSDoc comments for functions, interfaces, enums, and classes
- Explain "why" not "what" when the code is self-explanatory
- Document complex logic and business rules

### UI Labels

- Use title-style capitalization for commands, buttons, and menu items
- Don't capitalize prepositions of four or fewer letters unless first or last word

### Code Quality

- All files must include Microsoft copyright header
- All user-facing messages must be localized using `nls.localize()`
- Prefer regex capture groups with names over numbered groups
- Look for existing test patterns before creating new structures
- Use `describe` and `test` consistently with existing patterns
- Don't add tests to wrong test suite

### Best Practices

- Read files before modifying them
- Understand existing code and patterns before making changes
- Keep solutions simple - avoid over-engineering
- Only make changes directly requested or clearly necessary
- Don't add unnecessary features, refactoring, or "improvements"
- Don't add error handling for scenarios that cannot happen
- Trust internal code and framework guarantees
- Validate only at system boundaries (user input, external APIs)
- Delete unused code completely - no backwards-compatibility hacks

## Extension Development

### Extension Structure

```
extension-name/
├── package.json          # Extension manifest
├── src/
│   ├── main.ts          # Node.js entry point (desktop)
│   └── browser.ts       # Browser entry point (web)
└── README.md
```

### Extension Manifest (`package.json`)

```json
{
  "name": "extension-name",
  "main": "./out/main.js",
  "browser": "./dist/browser.js",
  "activationEvents": ["onLanguage:typescript"],
  "contributes": {
    "commands": [...],
    "languages": [...],
    "configuration": [...]
  }
}
```

### Extension API

- **Stable API**: `src/vscode-dts/vscode.d.ts`
- **Proposed API**: `src/vscode-dts/vscode.proposed.*.d.ts`
- Extensions run in isolated extension host process
- Access via `import * as vscode from 'vscode'`

## Common Development Workflows

### Adding a New Service

1. Define interface in `src/vs/platform/myservice/common/myservice.ts`
2. Create implementation(s) in platform-specific folders (`browser/`, `node/`)
3. Use `createDecorator<IMyService>('myService')` for DI
4. Register in appropriate `ServiceCollection`
5. Inject via constructor: `@IMyService private myService: IMyService`

### Adding a Workbench Contribution

1. Create feature in `src/vs/workbench/contrib/myfeature/`
2. Import in `src/vs/workbench/workbench.common.main.ts` or `.desktop.main.ts`
3. Register commands via `CommandsRegistry.registerCommand()`
4. Register menu items via `MenuRegistry.appendMenuItem()`
5. Register keybindings via `KeybindingsRegistry.registerKeybindingRule()`

### Adding Extension API

1. Add to `src/vscode-dts/vscode.d.ts` (stable) or `.proposed.*.d.ts` (proposed)
2. Implement in `src/vs/workbench/api/` (ExtHost* files)
3. Expose via extension host service
4. Update API tests in `extensions/vscode-api-tests/`

## Development Container

The repository includes DevContainer/Codespaces support:
- Requires Docker with 4 cores and 8GB RAM (9GB recommended)
- VNC server on port 5901 (password: `vscode`)
- Web client on port 6080
- Use `npm i && ./scripts/code.sh` to build and run
- Pre-configured with all required dependencies

## Performance Considerations

- Features load lazily via dynamic imports
- Heavy computation offloaded to web workers
- Disposable pattern prevents memory leaks
- Aggressive caching of compiled modules
- Code splitting for faster load times

## Important Notes

- The build system uses Gulp tasks orchestrated via `build/gulpfile.ts`
- Tests are organized by type: unit (`test/unit/`), integration (`test/integration/`), smoke (`test/smoke/`)
- The CLI is written in Rust (`cli/src/`) with Node.js wrapper
- Web version uses same codebase with different entry points
- Remote development uses separate server process with RPC communication
