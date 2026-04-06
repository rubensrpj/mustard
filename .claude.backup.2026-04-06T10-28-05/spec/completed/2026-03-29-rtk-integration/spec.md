# Feature: RTK Integration (Core)

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-03-29T02:00:00Z

## Summary

Integrar RTK (Rust Token Killer) como infraestrutura core do Mustard para economia de tokens.
- `mustard init` / `mustard update` instalam RTK silenciosamente (transparente)
- Hook PreToolUse reescreve Bash commands via `rtk` (fail-open)
- Statusline mostra economia de tokens em tempo real
- Pipeline CLOSE exibe relatório final de tokens economizados

## Checklist

### templates-impl Agent (Wave 1 — Hook + Settings + CLAUDE.md)

- [x] Criar `templates/hooks/rtk-rewrite.js` — PreToolUse hook:
  - Lê stdin JSON, extrai `tool_input.command`
  - Verifica se `rtk` existe no PATH (cache resultado em arquivo tmp para evitar check repetido; TTL 60s)
  - Cross-platform: `where rtk` (win32), `which rtk` (unix) via `execFileSync`
  - Se RTK disponível e comando NÃO começa com `rtk `: retorna `{ hookSpecificOutput: { hookEventName: "PreToolUse", permissionDecision: "allow", updatedInput: { command: "rtk <original>" } } }`
  - Se RTK indisponível ou comando já usa `rtk`: `process.exit(0)` (pass-through)
  - Fail-open em qualquer erro (try/catch → exit 0)
  - Node.js built-ins only, zero dependências
- [x] Registrar hook em `templates/settings.json` — PreToolUse Bash, PRIMEIRO na lista (antes de bash-safety.js — RTK rewrite primeiro, safety check depois)
- [x] Atualizar `templates/CLAUDE.md` — Adicionar seção "Token Economy" com info RTK

### templates-impl Agent (Wave 2 — Statusline + Pipeline CLOSE)

- [x] Modificar `templates/scripts/statusline.js` — Novo segmento RTK entre Duration e Lines:
  - Executar `rtk gain --all --format json` (com cache em tmp, TTL 30s para não chamar a cada render)
  - Extrair `saved_tokens` e `savings_pct` do JSON
  - Renderizar: `🞂 {savings_pct}% │ {saved_tokens_k}k saved` em verde se >50%, amarelo se >20%, cinza se 0
  - Se RTK não instalado ou erro: não mostrar segmento (graceful skip)
- [x] Modificar `templates/commands/mustard/complete/SKILL.md` — Adicionar ao output visual do CLOSE:
  ```
  Token Economy: {saved}k tokens saved ({pct}% reduction) — powered by RTK
  ```
  Instruir o agente a executar `rtk gain --all --format json` e extrair métricas

### templates-impl Agent (Wave 3 — CLI init/update)

- [x] Criar função `ensureRtk()` em `src/commands/init.ts`:
  - Detectar se `rtk` está no PATH (`which`/`where` conforme OS)
  - Se encontrado: `✓ RTK v{version} (token economy active)` em verde
  - Se NÃO encontrado — instalar silenciosamente:
    - Linux/macOS: `curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/refs/heads/master/install.sh | sh`
    - Windows: `cargo install --git https://github.com/rtk-ai/rtk` (se cargo disponível), senão imprimir instrução manual com URL do release
  - Após instalação: verificar com `rtk --version`
  - Se instalação falhar: `⚠️ RTK not installed — token economy will activate when RTK is available` (não bloqueia)
  - Usar ora spinner durante instalação
- [x] Chamar `ensureRtk()` em `initCommand()` após `ensureGlobalPermissions()`
- [x] Importar e chamar `ensureRtk()` em `updateCommand()` após re-copy de templates
- [x] Build + type-check (`npm run build`)

## Files (~7)

- `templates/hooks/rtk-rewrite.js` (create)
- `templates/settings.json` (modify — add hook entry first in PreToolUse Bash list)
- `templates/CLAUDE.md` (modify — add Token Economy section)
- `templates/scripts/statusline.js` (modify — add RTK gain segment)
- `templates/commands/mustard/complete/SKILL.md` (modify — add token report to CLOSE)
- `src/commands/init.ts` (modify — add ensureRtk)
- `src/commands/update.ts` (modify — import + call ensureRtk)

## Dependencies

- Wave 1 (hook) → independente
- Wave 2 (statusline + CLOSE) → independente de Wave 1
- Wave 3 (CLI) → independente de Wave 1 e 2
- Todas as waves podem rodar em paralelo

## Install Commands Reference

```bash
# Linux/macOS (silent)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/rtk/refs/heads/master/install.sh | sh

# Windows (via cargo)
cargo install --git https://github.com/rtk-ai/rtk

# Windows (manual — GitHub Releases)
# https://github.com/rtk-ai/rtk/releases → rtk-x86_64-pc-windows-msvc.zip

# Verify
rtk --version
rtk gain  # confirms correct RTK (not Rust Type Kit)
```

## RTK Gain JSON Format

```bash
rtk gain --all --format json
# Returns: { saved_tokens, input_tokens, output_tokens, savings_pct, commands_count, ... }
```

## References

- RTK GitHub: https://github.com/rtk-ai/rtk
- RTK Install: https://github.com/rtk-ai/rtk/blob/master/INSTALL.md
- RTK Audit Guide: https://github.com/rtk-ai/rtk/blob/master/docs/AUDIT_GUIDE.md
- Hook protocol: `updatedInput` + `permissionDecision: "allow"` (Claude Code v2.0.10+)
- Padrão hooks: `templates/hooks/bash-safety.js`
- Statusline: `templates/scripts/statusline.js`
