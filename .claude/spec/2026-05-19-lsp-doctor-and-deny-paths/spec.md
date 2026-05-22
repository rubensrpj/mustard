# Enhancement: LSP no doctor + permissions.deny declarativa para paths ruidosos

completed
### Stage: Close
### Outcome: Active
### Flags: 
### Scope: light
### Checkpoint: 2026-05-19T22:25:00Z
### Lang: pt

## PRD

## Contexto

O post _How Claude Code Works in Large Codebases_ trata LSP e exclusões declarativas como dois dos pilares do harness. Mustard hoje não detecta language servers (zero menção em `apps/rt/src/run/doctor.rs` e em `apps/cli/templates/`) e usa `permissions.deny` (`apps/cli/templates/settings.json:55-69`) apenas para comandos destrutivos — a exclusão de paths ruidosos (`node_modules`, `dist`, `bin`, …) vive só como instrução em CLAUDE.md, sujeita a drift. O resultado é que o `doctor` não avisa quando rust-analyzer/tsserver faltam e o Claude Code consumidor pode ler em pastas geradas que o próprio scanner já ignora. Esta spec fecha esses dois gaps mantendo a fonte única (`DEFAULT_IGNORE`) e o desenho agnóstico (sem hardcode novo de stack).

## Métrica de sucesso

`mustard-rt run doctor` em um projeto inicializado mostra uma linha `lsp` com status por stack detectada, e `apps/cli/templates/settings.json` nega leitura/grep em todos os diretórios listados em `DEFAULT_IGNORE`.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: doctor reporta um check `lsp` no output — Command: `node -e "const o=require('child_process').execSync('cargo run -q -p mustard-rt -- run doctor',{encoding:'utf8',stdio:['ignore','pipe','ignore']});if(!/lsp/i.test(o))process.exit(1)"`
- [x] AC-2: templates/settings.json nega Read e Grep em `node_modules` e `dist` (sentinelas do `DEFAULT_IGNORE`) — Command: `node -e "const j=require('./apps/cli/templates/settings.json');const d=j.permissions.deny.join('\n');const hits=['Read(**/node_modules/**)','Grep(**/node_modules/**)','Read(**/dist/**)','Grep(**/dist/**)'];process.exit(hits.every(h=>d.includes(h))?0:1)"`
- [x] AC-3: suíte rt continua verde — Command: `cargo test -p mustard-rt --quiet`

## Plano

## Sumário

Adiciona um 5º check ao `doctor` (`lsp`, fail-open, mapping stack→server derivado do `sync-detect`) e estende `permissions.deny` no template de `settings.json` com pares `Read(**/<dir>/**)` + `Grep(**/<dir>/**)` para cada entrada de `DEFAULT_IGNORE` (`apps/rt/src/run/scan/file_utils.rs:12`). Sem mudanças em CLI runtime; sem nova entity; sem migração.

## Checklist

### rt Agent

- [x] Em `apps/rt/src/run/doctor.rs`, adicionar `fn lsp_check() -> CheckResult` que: (1) chama a mesma rota de descoberta usada pelo `sync-detect` para obter as stacks ativas; (2) mapeia stack → binário esperado (rust → `rust-analyzer`, typescript/javascript → `typescript-language-server`, python → `pyright`, go → `gopls`, java → `jdtls`, csharp → `omnisharp`); (3) probes via lookup no `PATH` (reutilizar o helper de probe shell-out de `cli-failopen-pattern` se aplicável); (4) status `Ok` se todos presentes, `Warn` por server faltando com one-line install hint, `Skip` se nenhuma stack mapeada. Sem hardcode de stack — se `sync-detect` não conhece, `Skip`
- [x] Wirear `lsp_check` na lista principal de checks do `doctor` (junto a wiring/residue/drift/state)
- [x] Estender o módulo `#[cfg(test)] mod tests` (`apps/rt/src/run/doctor.rs:641+`) com 1-2 testes: (a) report contém um check chamado `"lsp"`; (b) com stack desconhecida, status = `Skip`
- [x] `cargo build -p mustard-rt && cargo test -p mustard-rt --quiet` — verde

### cli Agent

- [x] Em `apps/cli/templates/settings.json`, estender `permissions.deny` com pares `Read(**/<dir>/**)` e `Grep(**/<dir>/**)` para cada entrada de `DEFAULT_IGNORE` (`apps/rt/src/run/scan/file_utils.rs:12`): `node_modules`, `bin`, `obj`, `dist`, `.next`, `__pycache__`, `.venv`, `venv`, `target`, `build`, `migrations`, `Migrations`. Manter as entradas destrutivas existentes; só adicionar
- [x] Validar que `templates/settings.json` continua JSON válido após o edit — Command: `node -e "JSON.parse(require('fs').readFileSync('apps/cli/templates/settings.json','utf8'))"`

## Arquivos (~3)

- `apps/rt/src/run/doctor.rs` — novo `lsp_check` + wire + testes
- `apps/cli/templates/settings.json` — entradas adicionais em `permissions.deny`
- (sem 3º arquivo — testes ficam inline no doctor)

## Limites

Tocar apenas os dois arquivos acima. **Não** modificar: `sync-detect` (já entrega o que precisamos), CLI runtime (init/update copia o template verbatim), root `CLAUDE.md` (a remoção do bloco "Ignore Paths" do template já foi feita antes; o root é instrução para o repo Mustard em si). **Não** adicionar skill nova, hook novo, ou run subcommand novo — o doctor existente absorve a verificação.
