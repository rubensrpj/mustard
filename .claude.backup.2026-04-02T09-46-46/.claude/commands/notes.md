# Notes: Templates (General)

> Manual notes for the Mustard templates subproject. Never overwritten by /scan.

## Mandatory Patterns

- All hooks read JSON from stdin and write JSON to stdout
- All hooks fail-open (exit 0 on error) except when explicitly blocking
- All generated files start with `<!-- mustard:generated -->` header
- Skills use YAML frontmatter with `name` and `description` fields

## Known Pitfalls

- Hook stdin must be fully consumed before processing (`on('end', ...)`)
- Windows path separators must be normalized (`\` to `/`) in file-guard and guard-verify
- CLAUDE_PROJECT_DIR env var may not be set in all contexts — always fallback to cwd

## Observations

- Templates are copied verbatim to target projects by `mustard init`/`mustard update`
- The `settings.json` defines the full hook wiring — any new hook must be registered there
- Skills in `skills/` are foundation skills (stack-agnostic); subproject-specific skills go in `{sub}/.claude/skills/`
