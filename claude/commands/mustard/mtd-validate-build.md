# /mtd-validate-build - Validação de Build

> Executes build and type-check across all detected projects.

## Usage

```
/mtd-validate-build
/mtd-validate-build --project=<name>
```

## What It Does

1. **Detects** all projects in the workspace
2. **Executes** appropriate build/check commands per stack
3. **Reports** errors found

## Stack Detection

The command automatically detects projects by their manifest files:

| Manifest File | Stack | Build Command |
|---------------|-------|---------------|
| `package.json` | Node.js | `npm run build` / `pnpm build` / `yarn build` |
| `tsconfig.json` | TypeScript | `tsc --noEmit` |
| `*.csproj` | .NET | `dotnet build` |
| `*.sln` | .NET Solution | `dotnet build` |
| `requirements.txt` | Python | `python -m py_compile` |
| `pyproject.toml` | Python | `python -m build --check` |
| `go.mod` | Go | `go build ./...` |
| `Cargo.toml` | Rust | `cargo build` |
| `pom.xml` | Java (Maven) | `mvn compile` |
| `build.gradle` | Java (Gradle) | `gradle build` |

## Package Manager Detection (Node.js)

| Lock File | Package Manager |
|-----------|-----------------|
| `pnpm-lock.yaml` | pnpm |
| `yarn.lock` | yarn |
| `package-lock.json` | npm |
| (none) | npm (default) |

## Monorepo Support

The command detects and handles monorepo structures:

| Monorepo Type | Detection | Build Command |
|---------------|-----------|---------------|
| pnpm workspaces | `pnpm-workspace.yaml` | `pnpm build` or `pnpm run build --recursive` |
| yarn workspaces | `workspaces` in package.json | `yarn workspaces run build` |
| npm workspaces | `workspaces` in package.json | `npm run build --workspaces` |
| Lerna | `lerna.json` | `lerna run build` |
| Nx | `nx.json` | `nx run-many --target=build` |
| Turborepo | `turbo.json` | `turbo run build` |
| .NET Solution | `*.sln` | `dotnet build Solution.sln` |
| Cargo workspace | `[workspace]` in Cargo.toml | `cargo build --workspace` |
| Go workspace | `go.work` | `go build ./...` |

### Monorepo Flow

```
/mtd-validate-build
     │
     ├── Check for monorepo markers
     │   ├── pnpm-workspace.yaml / turbo.json / nx.json?
     │   ├── *.sln?
     │   └── go.work?
     │
     ├── If monorepo detected:
     │   └── Run workspace-aware build command
     │
     └── If not monorepo:
         └── Build each project individually
```

## Flow (Single Projects)

```
/mtd-validate-build
     │
     ├── Detect projects via Glob
     │   ├── **/package.json
     │   ├── **/*.csproj
     │   ├── **/go.mod
     │   └── ...
     │
     ├── For each project:
     │   ├── Identify stack
     │   ├── Run build command
     │   └── Collect output
     │
     └── Report results
```

## Arguments

| Argument | Description |
|----------|-------------|
| (none) | Validate all detected projects |
| `--project=<name>` | Validate specific project folder |

## Examples

```bash
# Validate all projects
/mtd-validate-build

# Validate specific project
/mtd-validate-build --project=api
/mtd-validate-build --project=web
```

## Output

### All OK

```
🔍 Validating projects...

📦 api/ (dotnet build)
   ✅ Build succeeded
   ⚠️ 0 warnings

📦 web/ (pnpm build)
   ✅ Build succeeded

📦 cli/ (go build)
   ✅ Build succeeded

━━━━━━━━━━━━━━━━━━━━━━━━━━
✅ All projects valid
```

### With Errors

```
🔍 Validating projects...

📦 api/ (dotnet build)
   ❌ Build failed

   Error CS1002: ; expected
   at Services/UserService.cs:142

📦 web/ (tsc --noEmit)
   ❌ Type errors

   error TS2339: Property 'email' does not exist
   at hooks/useUser.ts:23

━━━━━━━━━━━━━━━━━━━━━━━━━━
❌ Errors found: 2
```

## Notes

- Read-only: does not modify files
- Useful before commit/push
- Automatically executed by @review agent
