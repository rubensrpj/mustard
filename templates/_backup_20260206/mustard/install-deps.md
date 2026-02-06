# /install-deps - Instalar Dependencias

> Installs dependencies across all detected projects.

## Usage

```
/install-deps
/install-deps --project=<name>
```

## What It Does

1. **Detects** all projects in the workspace by manifest files
2. **Identifies** the appropriate package manager
3. **Executes** install commands for each project

## Project Detection

| Manifest File | Stack | Install Command |
|---------------|-------|-----------------|
| `package.json` | Node.js | `npm install` / `pnpm install` / `yarn` |
| `*.csproj` | .NET | `dotnet restore` |
| `*.sln` | .NET Solution | `dotnet restore` |
| `requirements.txt` | Python | `pip install -r requirements.txt` |
| `pyproject.toml` | Python | `pip install .` or `poetry install` |
| `go.mod` | Go | `go mod download` |
| `Cargo.toml` | Rust | `cargo fetch` |
| `pom.xml` | Java (Maven) | `mvn dependency:resolve` |
| `build.gradle` | Java (Gradle) | `gradle dependencies` |
| `Gemfile` | Ruby | `bundle install` |
| `composer.json` | PHP | `composer install` |

## Package Manager Detection (Node.js)

| Lock File | Package Manager |
|-----------|-----------------|
| `pnpm-lock.yaml` | pnpm |
| `yarn.lock` | yarn |
| `package-lock.json` | npm |
| (none) | npm (default) |

## Monorepo Support

The command detects and handles monorepo structures:

| Monorepo Type | Detection | Behavior |
|---------------|-----------|----------|
| pnpm workspaces | `pnpm-workspace.yaml` | Run `pnpm install` at root only |
| yarn workspaces | `workspaces` in root package.json | Run `yarn` at root only |
| npm workspaces | `workspaces` in root package.json | Run `npm install` at root only |
| Lerna | `lerna.json` | Run `lerna bootstrap` or `npx lerna bootstrap` |
| Nx | `nx.json` | Run `npm install` at root |
| Turborepo | `turbo.json` | Run package manager at root |
| .NET Solution | `*.sln` | Run `dotnet restore` on solution file |
| Cargo workspace | `[workspace]` in Cargo.toml | Run `cargo fetch` at root |
| Go workspace | `go.work` | Run `go mod download` at root |

### Monorepo Flow

```
/install-deps
      │
      ├── Check for monorepo markers at root
      │   ├── pnpm-workspace.yaml?
      │   ├── lerna.json?
      │   ├── nx.json?
      │   ├── *.sln?
      │   └── go.work?
      │
      ├── If monorepo detected:
      │   └── Install at root level only
      │
      └── If not monorepo:
          └── Install each project individually
```

## Flow (Single Projects)

```
/install-deps
      │
      ├── Glob for manifest files
      │   ├── **/package.json
      │   ├── **/*.csproj
      │   ├── **/requirements.txt
      │   ├── **/go.mod
      │   └── ...
      │
      ├── For each project:
      │   ├── Detect package manager
      │   └── Run install command
      │
      └── Report results
```

## When to Use

- After `git pull` with changes to manifest files
- After adding new dependencies
- When build fails due to missing dependency
- Initial environment setup

## Options

```
/install-deps                     # All detected projects
/install-deps --project=api       # Specific project folder
/install-deps --project=web       # Specific project folder
```

## Output

```
📦 Installing dependencies...

[api/] dotnet restore... ✅
[web/] pnpm install... ✅
[cli/] go mod download... ✅
[scripts/] pip install -r requirements.txt... ✅

━━━━━━━━━━━━━━━━━━━━━━━━━━
✅ All dependencies installed successfully
```

## Error Handling

```
📦 Installing dependencies...

[api/] dotnet restore... ✅
[web/] pnpm install... ❌
   error: Cannot find module '@types/react'

━━━━━━━━━━━━━━━━━━━━━━━━━━
❌ Some dependencies failed to install
```

## Notes

- Skips `node_modules`, `vendor`, `target`, `bin`, `obj` folders when scanning
- Respects `.gitignore` patterns
- Monorepos are detected automatically and handled at root level
- Mixed monorepos (e.g., pnpm + .NET) are supported - each workspace type is handled separately
