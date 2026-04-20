# Plano: `/scan --force` realmente regenera skills do zero em monorepos

## Context

O usuário rodou `/scan --force` em `C:\Atiz\Competi\projetos\sialia` (monorepo). Resultado:

- Pastas `skills/` dos subprojetos **não foram recriadas** do zero.
- Vários arquivos ficaram **vazios**.
- Expectativa: `--force` deveria apagar tudo o que é `mustard:generated` e regerar.

A investigação mostra que o problema é real e combina três causas independentes. Abaixo o diagnóstico e a correção mínima.

---

## Root causes (confirmados)

### 1) O próprio `/scan` não reconhece `--force`

`templates/commands/mustard/scan/SKILL.md` linha 7 (Trigger) documenta apenas:

```
/scan  ou  /scan <subproject>
```

Nenhum lugar do SKILL.md parseia `--force` vindo do usuário. Consequência: o orchestrator segue o caminho **incremental** (Step C, linhas 45-52): compara `sourceHashes` antigos × novos e **pula subprojetos cujo hash não mudou**. Em monorepo estável, a maioria dos subprojetos é pulada — inclusive na intenção explícita do usuário de “força bruta”.

O único `--force` do fluxo aparece em etapas internas (`sync-registry.js --force`, `skill-generator.js --force`, linhas 334-335), que **não recebem** o sinal vindo do usuário.

### 2) Task agents (Step 3 / §4.6) não apagam `skills/` antes de regenerar

Instrução atual (linhas 205-213) manda:

- backup de `commands/` para `_backup/`
- gerar skills granulares em `{subproject}/.claude/skills/{skill-name}/`

**Não** manda apagar `{subproject}/.claude/skills/` antes. Se o scan anterior criou `skills/api-foo/` e o novo agente não o recria, a pasta velha fica lá — às vezes com SKILL.md esqueleto/vazio. Qualquer mudança na taxonomia de skills entre scans deixa lixo.

### 3) `skill-generator.js --force` apenas sobrescreve; nunca deleta; e tolera conteúdo vazio

`.claude/scripts/skill-generator.js`:

- linha 36: `const FORCE = args.includes('--force');`
- linhas 91-95: guarda `isMustardGenerated` — com `--force` é ignorada; **sem** `--force`, arquivo com header é sobrescrito mesmo assim. `--force` hoje só cobre o caso do arquivo **não** ter o header.
- linhas 97-103: `writeFile` grava o que recebeu, sem checar se o conteúdo resultante é apenas frontmatter + header (ou seja, “vazio semântico”).
- função `genEntityCreationSkill()` (linha 281+): quando `_patterns.{stack}.*` do registry está nulo (comum em primeira execução ou em monorepo com registry só v3.x), a skill gerada vira estrutura sem exemplos — daí os “arquivos vazios”.

Nenhuma etapa **deleta** diretórios inteiros de skills `mustard:generated` antes de escrever.

---

## Recommended approach (mínimo e cirúrgico)

Uma única semântica: **`--force` = "descarta tudo `mustard:generated` e regera do zero, ignorando cache incremental"**.

### Mudança A — Propagar `--force` a partir do `/scan`

Arquivo: `templates/commands/mustard/scan/SKILL.md`

1. Atualizar **Trigger** (linha 7):
   - Aceitar `/scan`, `/scan <subproject>`, `/scan --force`, `/scan <subproject> --force`.
2. Adicionar uma subseção **“Flags”** logo abaixo de Trigger definindo `--force`:
   - pula o Step C (incremental skip) — sempre processa todos os subprojetos;
   - remove o fast-path de 2.6 (Bootstrap) se `--force`;
   - repassa `--force` para os agentes do Step 3 e para `skill-generator.js`.
3. No Step 1/C (linhas 45-52), adicionar condição: “se `--force`, ignorar comparação de hash e marcar todos os subprojetos como `needs-rescan`”.
4. No Step 3 (linhas 195-213), acrescentar ao prompt do Task agent (apenas quando `--force`):
   ```
   FORCE MODE: before generating skills, delete every directory in
   {path}/.claude/skills/ whose SKILL.md contains "<!-- mustard:generated".
   Preserve directories without that marker (user-authored skills).
   ```
5. No §4.7 (linhas 333-336), manter `skill-generator.js --force` — mas ver Mudança B.

### Mudança B — `skill-generator.js`: modo `--force` deleta antes de gerar + valida conteúdo

Arquivo: `.claude/scripts/skill-generator.js`

1. Em `buildStackSubprojectMap()` (linha 176) — após resolver cada subprojeto, se `FORCE`, chamar um novo helper `purgeGeneratedSkills(subAbsPath, log)`:
   - varre `{subAbsPath}/.claude/skills/*/SKILL.md`
   - se `isMustardGenerated(file)` → `fs.rmSync(dir, { recursive: true, force: true })`
   - loga `[purge] {rel}` por diretório removido
   - preserva skills sem o header (user-authored)
2. Em `writeFile` (linhas 83-104), antes do `log.push('[write] …')`, validar que o conteúdo tem **corpo útil** (>200 caracteres **depois** do frontmatter + header). Se não tiver:
   - não grava
   - empurra `[skip-empty] {rel} — pattern data missing`
   - acumula em array `emptySkipped` para o relatório final.
3. Ao fim do script, se `emptySkipped.length > 0` e **não** for `DRY_RUN`, escrever em stderr uma seção `Empty skill bodies (pattern data missing):` com a lista — para o orchestrator surfaciar no retorno do `/scan`.
4. Guard de v4: o warning atual (linha ~2011, `Warning: registry at v${version}`) passa a ser fatal quando `FORCE` está ativo e `version < 4.0` — evita o caso “regerei tudo mas sem padrões, ficou vazio”. Exit code não-zero com mensagem clara; `scan.md` já manda `sync-registry.js --force` antes (linha 334), então a pré-condição está garantida no fluxo feliz.

### Mudança C — Step 4.7 do scan: rodar `sync-registry.js --force` **sempre** que `--force`

Hoje já roda em 4.7, mas o fast-path de 2.6 (linha 101) pode pular bootstrap. Adicionar à seção Flags: “com `--force`, sempre executar os dois comandos de 4.7, mesmo que o registry já exista e seja v4.0”.

---

## Arquivos a modificar

| Arquivo | Mudança |
|---|---|
| `templates/commands/mustard/scan/SKILL.md` | Trigger, seção Flags, Step 1/C, Step 3, §2.6 fast-path, §4.7 |
| `.claude/scripts/skill-generator.js` | `purgeGeneratedSkills` + validação de corpo vazio + v4 fatal sob `--force` |

Arquivos **não** alterados de propósito:

- `sync-detect.js` — `--no-cache` já cobre a parte de cache dele; `--force` do scan é semântica distinta (não-incremental + purge).
- `sync-registry.js` — já tem `--force` próprio e é chamado explicitamente em §4.7.

---

## Funções/utilidades a reaproveitar

- `isMustardGenerated(filePath)` (skill-generator.js:64-73) — já identifica arquivos regeneráveis. Usar no novo `purgeGeneratedSkills`.
- `readJsonSafe`, `writeFile` (skill-generator.js:51, 83) — manter, estender `writeFile` com a checagem de corpo.
- `buildStackSubprojectMap` (skill-generator.js:176) — é onde a iteração por subprojeto já acontece; o purge entra dentro desse loop.

---

## Verification (end-to-end)

1. **Unit-ish no Mustard**: criar um fixture monorepo mínimo com dois subprojetos, cada um com `skills/{stale}/SKILL.md` marcado `mustard:generated` e `skills/{user}/SKILL.md` sem o header.
   - Rodar `node .claude/scripts/skill-generator.js --force` (com registry v4 mockado).
   - Confirmar: `stale/` removido, `user/` preservado, novas skills geradas.
   - Rodar de novo sem `--force`: nada deletado.
2. **Smoke real**: no próprio repo Mustard, `node .claude/scripts/skill-generator.js --dry-run --force` não deve crashar nem remover nada (dry-run).
3. **Regressão no projeto do usuário** (`C:\Atiz\Competi\projetos\sialia`):
   - `git status` limpo antes.
   - Rodar `/scan --force`.
   - Conferir: cada `{subproject}/.claude/skills/` tem apenas diretórios recém-criados (timestamps novos no header) e nenhum SKILL.md tem corpo <200 chars.
   - Stderr do `skill-generator.js` sem entradas `[skip-empty]`; se houver, o `/scan` surfacia no bloco `errors` do JSON final.
4. **Hooks/tests** do Mustard:
   - `node --test hooks/__tests__/hooks.test.js` continua passando (mudança não toca hooks).
