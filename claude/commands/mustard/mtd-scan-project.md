# /mtd-scan-project - Escanear Projeto

> Scans the project and detects stacks, patterns, and entities.

## Usage

```
/mtd-scan-project
```

## What It Does

1. **Detects** stacks (languages, frameworks)
2. **Analyzes** folder structure
3. **Maps** existing entities
4. **Identifies** project patterns
5. **Generates** entity-registry.json

## Flow

```
/mtd-scan-project
     │
     ▼
Detect manifest files
(package.json, *.csproj, go.mod, etc)
     │
     ▼
Identify stacks
     │
     ▼
Analyze structure
     │
     ▼
Map entities
     │
     ▼
Generate outputs
```

## Stack Detection

| Manifest File | Detected Stack |
|---------------|----------------|
| `package.json` | Node.js (+ framework from deps) |
| `tsconfig.json` | TypeScript |
| `*.csproj` | .NET |
| `go.mod` | Go |
| `Cargo.toml` | Rust |
| `requirements.txt` | Python |
| `pyproject.toml` | Python |
| `pom.xml` | Java (Maven) |
| `build.gradle` | Java (Gradle) |

## Output

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📊 SCAN: {ProjectName}
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

🔧 Detected Stacks
├── {detected_stack_1}
├── {detected_stack_2}
└── {detected_stack_3}

📁 Structure
├── {project_folder_1}/
│   └── {detected_pattern}/
├── {project_folder_2}/
│   └── {detected_pattern}/
└── {project_folder_3}/
    └── {detected_pattern}/

📦 Mapped Entities: {count}
├── New: {new_count}
├── Modified: {modified_count}
└── Removed: {removed_count}

📐 Detected Patterns
├── Naming: {detected_naming_convention}
├── Soft Delete: {yes/no}
├── Multi-tenant: {yes/no}
└── Other: {other_patterns}

📄 Generated Files
├── .claude/project.json
└── .claude/entity-registry.json

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

## Generated Files

### project.json

```json
{
  "name": "{ProjectName}",
  "stacks": {
    "primary": "{detected_primary_stack}",
    "secondary": "{detected_secondary_stack}"
  },
  "structure": {
    "src": "{detected_src_pattern}",
    "tests": "{detected_test_pattern}"
  },
  "patterns": {
    "softDelete": false,
    "multiTenant": false,
    "naming": "{detected_naming}"
  }
}
```

### entity-registry.json

```json
{
  "_v": "2.1",
  "_p": {
    "src": "{detected_src_path}/{e}",
    "test": "{detected_test_path}/{e}.test"
  },
  "e": {
    "Entity1": 1,
    "Entity2": 1,
    "Entity3": { "sub": ["SubEntity"] }
  }
}
```

## When to Use

- New project (first time)
- After major structural changes
- Periodically (weekly)
- Before complex features

## Notes

- Combines `/what-patterns` and parts of `/where-am-i`
- Updates entity-registry automatically
- Detects changes since last scan
