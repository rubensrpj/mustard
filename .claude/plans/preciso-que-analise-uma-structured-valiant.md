# Análise — Skills geradas no scan não devem inventar

## Contexto

Usuário relata que durante `/scan`, as skills geradas **inventam resoluções** em vez de seguir rigorosamente os padrões do projeto. Pede que o padrão **mais encontrado** prevaleça para cada skill.

Resultado da investigação: o Mustard **já tem regras anti-invenção** em `scan-format §10`, mas **não tem enforcement pós-geração**. Toda a responsabilidade está no prompt do Task agent (step 3 do scan), que pode:

1. Preencher `## Convention` com campos fora do `_patterns.discovered[]`
2. Listar `## Real examples` com paths que não existem no FS
3. Escrever fenced code em SKILL.md (proibido pela regra — código só em `references/examples.md`, extraído de arquivos reais)
4. Usar nome de skill com brand de library em vez de vocabulário de folder do codebase
5. Emitir skill de cluster com `<3 files` (deveria ser skip)

## Escopo

Scan gera skills via **Task agent (general-purpose)** lançado em [scan/SKILL.md:198-229](.claude/commands/mustard/scan/SKILL.md). Não existe skill-generator mecânico — confirmado em [scan/SKILL.md:353](.claude/commands/mustard/scan/SKILL.md:353).

O agente recebe o prompt em [scan/SKILL.md:205-229](.claude/commands/mustard/scan/SKILL.md) e deve ler `scan-format.md §10` (as regras). Nenhum gate valida o output.

## Arquivos críticos

| Arquivo | Papel |
|---|---|
| [.claude/commands/mustard/scan/SKILL.md](.claude/commands/mustard/scan/SKILL.md) | Prompt orquestrador + dispatch para Task agent |
| [.claude/commands/mustard/scan-format/SKILL.md:179-317](.claude/commands/mustard/scan-format/SKILL.md) | §10 — regras de decomposição, frequency gate, naming, NO CODE, samples reais |
| [.claude/scripts/sync-registry.js](.claude/scripts/sync-registry.js) | Produz `_patterns[stack].discovered[]` que alimenta as skills (source of truth) |
| [.claude/scripts/registry/cluster-discovery.js](.claude/scripts/registry/cluster-discovery.js) | Calcula clusters e `folderFrequency` (stopwords derivados do projeto) |
| [.claude/scripts/skill-validate.js](templates/scripts/skill-validate.js) | Já existe — valida frontmatter mas não evidência factual |

## Recomendação (subtrair > adicionar)

**Não criar gerador novo nem 3ª camada.** Endurecer o que já existe, em 2 frentes:

### Frente 1 — Hardening do prompt do Task agent (scan/SKILL.md step 3)

Adicionar ao prompt do agente, imediatamente após "Read scan-format.md":

```
EVIDENCE RULE — before emitting any skill:
1. Skill must correspond to a cluster in _patterns[stack].discovered[] with fileCount ≥ 3.
   Skill name suffix MUST equal the cluster's `suffix` (slugified). No renaming to library brands.
2. Every path listed under `## Real examples` or `## Samples in this project` must be
   confirmed via Glob/Read. Drop entries that don't exist on disk.
3. `## Convention` fields must be restricted to keys present in the cluster object
   (suffix, folders, fileCount, commonBaseClass, commonInterfaces). Do not add fields.
4. NO fenced code blocks in SKILL.md. All code goes to references/examples.md,
   extracted via Read from a real source file (verbatim, ≤80 lines).
5. If you cannot meet rules 1-4 for a candidate skill, SKIP it. Empty is better than invented.
```

### Frente 2 — Hook de validação pós-geração (skill-validate.js extendido)

Extender [templates/scripts/skill-validate.js](templates/scripts/skill-validate.js) para checks factuais por skill gerada (em `{subproject}/.claude/skills/*/`):

1. **Header gate**: SKILL.md tem `<!-- mustard:generated -->`. Skip user-authored.
2. **Frequency gate**: skill name suffix corresponde a cluster em `entity-registry.json._patterns[*].discovered[]` com `fileCount ≥ 3`. Se não, flag `NO_CLUSTER`.
3. **Existence gate**: toda linha `- {X} — \`{path}\`` em `## Real examples` / `## Samples`: Glob o path. Se não existe, flag `STALE_SAMPLE`.
4. **No-code gate**: rejeita SKILL.md com fenced blocks (` ``` `). Flag `CODE_IN_BODY`.
5. **References gate**: se `references/examples.md` existe, cada bloco `Source: \`{path}\`` deve existir no FS.

Saída: JSON com `{ skill, violations[] }`. Exit code 1 se alguma skill tem violação.

Chamar o validator ao final de `/scan` (nova seção `### 6. Validate Skills` em scan/SKILL.md):

```bash
node .claude/scripts/skill-validate.js --factual
```

Em modo strict (default), aborta com lista de skills inválidas. Em `warn`, só reporta. Controle: `MUSTARD_SKILL_VALIDATE_MODE=strict|warn|off`.

## Reuso (não recriar)

- `_patterns[*].discovered[]` do registry — já é a fonte canônica (vem de `sync-registry.js`)
- `folderFrequency` já funciona como stopword do projeto (cluster-discovery.js)
- `skill-validate.js` já existe e já tem a estrutura de leitura de frontmatter; extender, não reescrever

## Verificação

1. Rodar `/scan --force` em um subprojeto conhecido
2. Para cada skill em `{sub}/.claude/skills/`:
   - `grep -c '```' SKILL.md` deve ser `0`
   - Cada path em `## Real examples` deve abrir com `Read`
   - `skill.name` sem prefixo de library brand (React/Drizzle/Prisma), apenas vocabulário de folder
3. Rodar `node .claude/scripts/skill-validate.js --factual` — exit 0
4. Quebrar propositalmente: editar uma skill para listar `src/Fake/Ghost.cs` — validator deve falhar com `STALE_SAMPLE`
5. Comparar antes/depois de `/scan` no mesmo projeto: número de skills tende a **diminuir** (fim das skills inventadas) — isso é sinal de sucesso, não regressão

## Fora de escopo

- Não mexer em `sync-detect.js` scoring (já funciona; não é fonte da invenção)
- Não adicionar LLM/Ollama no pipeline de skill (seria vetor extra de alucinação)
- Não criar skill-generator mecânico separado — mantém arquitetura atual (agent + validator)
