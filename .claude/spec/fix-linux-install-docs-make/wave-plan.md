---
id: wave.fix-linux-install-docs-make.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.fix-linux-install-docs-make.1-installer]] | installer | — | Make install.sh fetch the .deb itself, survive being piped into sh, and expose a --dry-run that resolves without installing |
| 2 | [[wave.fix-linux-install-docs-make.2-docs]] | docs | [[wave.fix-linux-install-docs-make.1-installer]] | Rewrite the four Linux install texts around the one-liner and add the missing Claude Code plugin step |

## Acceptance Criteria
- AC-1 — when the installer is piped into sh with no package beside it, then it resolves the package from the release and reports what it would install, instead of aborting. Command: `bash -c "tr -d '\r' < packaging/installer/install.sh | sh -s -- --dry-run"`
- AC-3 — the project build passes green. Command: `cargo build --workspace`
- AC-2 — when the four install texts are checked, then each one carries the one-line curl command, and the Linux tutorial and both READMEs carry the concrete marketplace command. Command: `bash -c "grep -q 'releases/latest/download/install' packaging/installer/TUTORIAL-LINUX.md && grep -q 'plugin marketplace add rubensrpj/mustard' README.md"`
