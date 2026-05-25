---
name: claude-dir-audit
description: .claude/ no raiz pode acumular pastas/arquivos zumbi — auditoria periódica cruzando com uso real de rt/cli/dashboard é mandatória
metadata:
  type: principle
  origin_spec: 2026-05-25-mustard-deep-refactor
  origin_wave: wave-2-mixed
---

# .claude/ Dir Audit

A pasta `.claude/` no raiz de um projeto (que usa Mustard) pode acumular ao longo do tempo arquivos e subpastas que o usuário **não solicitou** — ferramentas antigas, caches stale, templates de versões anteriores. Sem auditoria periódica, vira lixo que polui graph view do Obsidian, output de `active-specs`, e injeção de boot da IA.

## Regra

Antes de declarar qualquer feature "pronta" no Mustard, cruzar o conteúdo de `.claude/` com 3 fontes:

1. O que `apps/rt/src/` realmente lê em runtime (paths, env vars)
2. O que `apps/cli/src/` instala/atualiza
3. O que `apps/dashboard/` consome via Tauri commands

Tudo que está em `.claude/` mas nenhuma das 3 toca = candidato a remoção.

## Origem

User pediu explicitamente em 2026-05-25 "verificação profunda agora sobre o que de fato o mustard usa, muitos arquivos e pastas que não tenho conhecimento". Auditoria nesta sessão removeu 8 paths ORPHAN/LEGACY (797 KB para backup). Mecanização sistemática vive em [[wave-2-mixed]] da [[2026-05-25-mustard-deep-refactor]] (`mustard-rt run claude-dir-prune` + janitor no SessionStart).

## Aplica-se a

- `mustard-rt run claude-dir-prune --dry-run` deve ser rodado periodicamente (idealmente automatizado via SessionStart hook).
- Saída deve ser tabela `path → used_by? → action (keep/stale/orphan/legacy)`.
- Apenas usado/referenciado por código vivo fica; resto vira candidato a remoção (com backup antes).

## Status

Active.

## Relacionado

- [[templates_md_moat]] — limpeza preserva qualidade do payload
