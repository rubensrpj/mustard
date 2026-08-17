# Change Log — private-install-mode-leaving-no

_Solicitações registradas automaticamente durante o pipeline (mid-spec). O `spec.md` (narrativa congelada) NÃO é alterado; dobre o que muda comportamento em `## Acceptance Criteria` e rode o QA de novo._

- **2026-08-17T20:26:42.906Z** _(Plan)_ — ar
- **2026-08-17T20:36:43.874Z** _(Plan)_ — segue
- **2026-08-17T21:10:17.538Z** _(Execute)_ — **Instruction:** AC-8's fixture must exercise the INTERACTIVE install path, not only the fresh one: seed the host repo with a pre-existing .claude/ so init takes its backup-and-overwrite branch and produces a .claude.backup.<stamp>/ directory. Wave 3 found that this directory is NOT covered by the root-anchored .claude/settings.json rule and taught hide_footprint to exclude it by discovery — but no acceptance criterion names that behaviour, so nothing verifies it. AC-8 already asserts git status --porcelain --untracked-files=all comes back EMPTY; running it over a fixture that contains a backup directory is what turns that assertion into the missing coverage. Do not add a new criterion for it - the behaviour is already green, so the negative proof a new criterion needs cannot be taken.
