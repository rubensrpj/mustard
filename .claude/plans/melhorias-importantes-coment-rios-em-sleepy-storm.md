# Coerência de linguagem: spec na Lang escolhida, código sempre EN

## Context

Hoje os agentes de implementação produzem comentários ora em inglês, ora em português. A regra do Mustard já está correta (`templates/refs/feature/spec-language.md:66`: *"Code/commands stay EN"*), mas:

- A frase é genérica ("code") — agentes interpretam como cobrindo só identificadores, e escapam comentários para a língua da spec.
- A regra está enterrada em apenas dois pontos (`spec-language.md` e `agent-prompt/SKILL.md`), nenhum dos quais é parte do prompt mais quente lido durante EXECUTE.
- `karpathy-guidelines` (lido por todo agente que edita código) não cita linguagem.

**Decisão (do user):** simplificar — **tudo dentro de código fica em inglês**. A spec é o único artefato apresentado na linguagem escolhida (`### Lang: pt|en`).

| Artefato | Linguagem |
|---|---|
| Spec narrativa (`## Contexto`, `## Resumo`, `## Concerns`, prosa em geral) | `Lang` |
| Headings (`## Tasks` / `## Tarefas`, etc.) | `Lang` (via tabela de tradução existente) |
| Concerns adicionados por agentes na spec | `Lang` |
| **Código-fonte (qualquer arquivo do projeto)** | **EN sempre** |
| **Comentários (`//`, `#`, `/* */`, `///`, `'''`, `"""`, JSDoc, `<!-- -->`)** | **EN sempre** |
| Doc-comments lidos pelo `description-enricher.js` (glossário) | **EN sempre** |
| Identificadores, file paths, shell commands, AC `Command:` field | EN sempre (já era) |

**Por que importa:** `templates/scripts/registry/description-enricher.js` (novo, untracked) lê doc-comments e popula `entity-registry.json#description`, fonte de `/mustard:knowledge glossary`. Comentários consistentes em EN = glossário consistente. Bonus: melhor compatibilidade com bibliotecas/IDEs/code-search e termos canônicos do domínio que já são EN.

**Restrições não-negociáveis:**

- Sem hook detector de idioma — `spec-language.md:13` veta heurística de stopword/diacritic.
- Surgical: agentes **não traduzem** comentários PT pré-existentes; só os comentários **novos** precisam estar em EN. Migração de legado é separada (nem sequer recomendada).

## Critical files (templates/ + espelhos .claude/)

| Arquivo | Edição |
|---|---|
| `templates/refs/feature/spec-language.md` | Renomear §"Always EN" para incluir comentários explicitamente; reforçar §"Dispatch Propagation" linha 66 |
| `templates/commands/mustard/templates/agent-prompt/SKILL.md` | Linha 23: reescrever para deixar inequívoco |
| `templates/skills/karpathy-guidelines/SKILL.md` | Anexar §5 "Code is always English" |
| `templates/commands/mustard/feature/SKILL.md` | Linha 99: depois da HARD RULE de headings, anexar HARD RULE de código sempre EN |
| `templates/commands/mustard/bugfix/SKILL.md` | Mesma HARD RULE (a regra de headings já existe ~93–101) |
| `.claude/refs/feature/spec-language.md` | Espelho |
| `.claude/commands/mustard/templates/agent-prompt/SKILL.md` | Espelho |
| `.claude/skills/karpathy-guidelines/SKILL.md` | Espelho |
| `.claude/commands/mustard/feature/SKILL.md` | Espelho |
| `.claude/commands/mustard/bugfix/SKILL.md` | Espelho |

## Conteúdo das edições

### 1. `agent-prompt/SKILL.md` linha 23 — reescrever (Dispatch Template)

```
3. Spec language is `{spec_lang}`.
   - Use `{spec_lang}` for: spec prose, Concerns you append, labels in spec.
   - Source code is ALWAYS English: identifiers, comments (//, #, /*, ''', """, doc-comments, <!-- -->), file paths, shell commands, AC `Command:` field, log messages — regardless of `{spec_lang}`.
   - Surgical: do not translate pre-existing comments; just write new ones in English.
```

### 2. `spec-language.md` — substituir §"Always EN" (linha 49–60) e §"Dispatch Propagation" (linha 63–67)

```markdown
## Always EN — covers ALL code

These stay in English regardless of `Lang`:

**Spec metadata (parsed by scripts):**
- Status values: `draft | implementing | completed | cancelled`
- Phase values: `PLAN | EXECUTE | QA | CLOSE | COORDINATE`
- Scope values: `light | extended-light | full`
- The `### Lang:` line itself (literal)
- Hook output prefixes (`[SPEC-SIZE]`, `[HYGIENE]`, `[BOUNDARY WARNING]`)

**Source code (every file the agent writes/edits):**
- Identifiers (variable, function, class, type, interface, enum names)
- File paths
- Shell commands and AC `Command:` field content
- **Comments** — every form: `//`, `#`, `/* */`, `///`, `//!`, `'''`, `"""`, JSDoc, JavaDoc, XML doc-comments, `<!-- -->`
- Log/error/exception messages
- API string constants the agent introduces (unless replacing an existing localized string)

**Hard rule:** `Lang` controls only spec narrative (prose, headings, Concerns). Source code never carries `{spec_lang}`. Agents must not switch their own writing style based on `Lang`.

**Surgical:** never translate pre-existing comments while editing a file (aligns with karpathy §3). New comments the agent writes are in English.

**Why:** `entity-registry.json#description` is populated by `scripts/registry/description-enricher.js` from doc-comments and feeds `/mustard:knowledge glossary`. EN-only comments = consistent glossary; mixed comments break it.

## Dispatch Propagation

Agent dispatch template (`templates/commands/mustard/templates/agent-prompt/SKILL.md`) receives `{spec_lang}` placeholder. Orchestrator reads the spec's `### Lang:` line and fills it. The CONTEXT block instructs:

```
Spec language is `{spec_lang}`. Use it for spec prose, labels, and any Concerns you append. Source code (identifiers, comments in every form, paths, commands, log messages) stays English regardless. Don't translate pre-existing comments.
```

Agents adding `## Concerns` or marking `[x]` boxes inherit `{spec_lang}` automatically. Code they write does not.
```

### 3. `karpathy-guidelines/SKILL.md` — anexar §5 antes da linha 70 (separador `---`)

```markdown
## 5. Code Is Always English

Source code — including identifiers, **comments in every form** (`//`, `#`, `/* */`, JSDoc, doc-comments, etc.), log/error messages, and API constants you introduce — stays in English. The spec's `### Lang:` value applies to spec narrative only, never to code.

If a comment is worth writing (and CLAUDE.md's "default to no comments" already discourages most), write it in English. Don't switch language because the user's chat or spec is in another language. Don't translate pre-existing comments while editing — surgical (§3) overrides.

This keeps doc-comments consistent across the codebase, so `/mustard:knowledge glossary` (fed by `description-enricher.js` from doc-comments) stays useful.
```

### 4. `feature/SKILL.md` linha 99 (depois da HARD RULE Headers) — anexar bullet

```
**HARD RULE — Source code language:** every file the agent writes/edits stays in English regardless of `Lang`. This covers identifiers, comments (`//`, `#`, `/* */`, doc-comments, JSDoc, `<!-- -->`), log messages, AC `Command:` content. `Lang` applies to spec narrative only. Pre-existing comments are NOT translated (surgical changes).
```

### 5. `bugfix/SKILL.md` — mesma HARD RULE inserida após a regra de headers existente (~linha 99–101)

(idêntico ao bullet acima)

## Verification

1. **Diff sanity** — após as edições:
   ```
   Grep for "Code/commands stay EN" in templates/ and .claude/{commands,refs,skills} → expect 0 hits (frase substituída)
   Grep for "Source code is ALWAYS English" or "stays English" → expect ≥6 hits (cinco arquivos x duas versões)
   ```

2. **Smoke pipeline** num subprojeto de teste:
   - Setar `### Lang: pt` numa spec ativa (ou `specLang: "pt"` em `mustard.json`).
   - Rodar `/mustard:feature` em caso Light que crie 1 arquivo com pelo menos 1 comentário.
   - Conferir no diff: a spec está em PT (Contexto, Resumo, Tarefas), e o arquivo de código tem comentários em EN.
   - Repetir com `Lang: en` para garantir simetria.

3. **Glossário** após o smoke:
   ```
   node scripts/sync-registry.js --force
   /mustard:knowledge glossary
   ```
   Confirmar que descriptions de entidades novas estão em EN.

4. **Tests existentes** — rodar `node --test templates/hooks/__tests__/hooks.test.js` e `templates/hooks/__tests__/integration.test.js` para garantir que nenhuma asserção textual sobre Dispatch Template quebrou.

## Out of scope (explicitamente)

- Migrar comentários PT legados — surgical (karpathy §3). Quem mexer no arquivo escreve novos em EN; legado fica.
- Hook validador de idioma — vetado pelo princípio agnóstico do Mustard.
- Mexer em UI strings/i18n da aplicação alvo — fora do escopo desta regra (i18n é localização de produto, não comentário de código).
- Tradução automática de doc-comments existentes do `description-enricher.js` — o enricher só lê o que está no arquivo; com a regra aplicada e arquivos novos, o glossário converge em EN naturalmente.
