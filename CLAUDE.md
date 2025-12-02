# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

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
