# /scan — Diagnóstico e Plano de Correção

## Context

`/scan` em sialia produziu **5 agents · 342 tool uses · ~573k tokens** (Sialia.Backend sozinho: 144.3k tokens em 109 tool uses). Dois agents (`partners`, `core`) retornaram **mid-flight** sem completar; o agent `core` gravou skills em `.claude/skillscore-*` na raiz do Mustard repo em vez de `sialia-core/.claude/skills/core-*`.

**Premissa não-negociável**: Mustard é o input do SDD. O scan precisa identificar bem pra que feature/bugfix downstream não erre. Cortar Reads sem mais nada = skills mínimas = agente downstream sem orientação real do codebase = bugs.

**Reframe**: o problema NÃO é "agent lê demais". É que cada um dos 5 agents lê informação **redundante** com o que o `cluster-discovery.js` já extrai (e pode extrair mais). O fix certo é mover trabalho de identificação do agent (caro, paralelo × 5, com latência de tool dispatch) pro discovery script (barato, uma vez, sem hooks).

## Root Causes (revisadas)

| # | Root cause | Evidência | Impacto |
|---|---|---|---|
| 1 | EVIDENCE RULE 3 força ≥3 Reads por cluster pra validar majoritariedade que o discovery já calculou implicitamente | `evidence-rules.md:18-22`, `agent-prompt.template.md:22` | ~30 Reads × 5 agents = 150 Reads/scan |
| 2 | Cluster object não inclui metadados que o agent precisa pra Convention fields (naming, modifiers, async, imports) — agent precisa Read pra extrair | `cluster-discovery.js:661-687` materializa só 5 fields | Reads compulsórios pra agent gerar skill útil |
| 3 | Cada agent faz Read no `entity-registry.json` inteiro pra iterar `_patterns[stack].discovered[]` | `agent-prompt.template.md:59` | 1 Read grande × 5 agents redundantes |
| 4 | Cada agent faz Read em sample.cs/ts pra extrair `references/examples.md` (até 80 linhas verbatim) | `evidence-rules.md:25-26`, `skill-generation.md:96-106` | 1 Read por skill (5-10 skills × 5 agents) |
| 5 | `scan-format/SKILL.md` aponta pra refs externos (`refs/scan/*.md`, ~10k tokens) com `→ See` que sugere obrigatório | `scan-format/SKILL.md:128, 134, 141` | Agent lê refs por safety, sem necessidade |
| 6 | Contradição "ALSO create root" vs "ONLY subproject" + falta de path absoluto no template = path bug | `refs/scan/skill-generation.md:134 vs 142`, `agent-prompt.template.md:5` usa `{{path}}` relativo | Skills gravadas no Mustard repo (`.claude/skillscore-*`) |
| 7 | Agent não tem heurística pra parar de explorar — atinge limite implícito do harness e retorna parcial | nenhum cap no template, observado: partners 87.9k/42 uses, core 100.2k/47 uses | Mid-flight returns silenciosos |

## Estratégia: Mover trabalho do agent → discovery script

**Princípio**: o `cluster-discovery.js` já abre arquivos pra fazer regex matching (linhas 534, 577, 622, 745). É **gratuito** ele extrair metadados extras na mesma passagem. O `orchestrate.js` pode injetar samples pre-lidos no agentPrompt. Resultado: skills mais ricas com menos Reads do agent.

## Recommended Fix — 7 mudanças coordenadas

### F1. Enriquecer cluster object via heurísticas universais (100% agnóstico)
**Arquivos**: `.claude/scripts/registry/cluster-discovery.js` + espelho `templates/scripts/registry/cluster-discovery.js`

**Princípio**: nada de `if (stackId === 'X')`. Nenhum keyword (`class`, `def`, `using`, `import`) hardcoded. Toda extração é heurística baseada em propriedades universais (nome do arquivo, indentação, primeiras linhas, separadores de case).

Para cada cluster, ler os samples (cap 3-5 já existente) e extrair:

| Campo | Heurística universal |
|---|---|
| `namingPattern` | Split case do basename (`_splitPascalCase` já existe). Reporta se cluster suffix está antes/depois do prefix. Ex: `UserService` → suffix `Service` vem depois; `ServiceUser` → vem antes. |
| `declarationKeywords` | Encontrar a linha onde aparece o token = basename do arquivo (case-insensitive, depois de normalizar split-case). Palavras ANTES do identifier nessa linha = keywords/modifiers. Top-3 combinações observadas nos samples. Vale pra `public sealed class UserService` (C#), `pub struct UserService` (Rust), `class UserService:` (Py), `type UserService struct` (Go). |
| `declarationSuffix` | Palavras DEPOIS do identifier na mesma linha. Captura `: BaseClass, IFoo` (C#), `extends Base implements I` (TS), `(Base):` (Py), `struct { ... }` (Go). |
| `topOfFileLines` | Primeiras 20 linhas não-vazias, não-começadas por chars de comentário comuns (`//`, `#`, `--`, `/*`, `;`). Interseção entre samples → top-5 linhas compartilhadas. Tipicamente captura imports/uses/package. |
| `memberSuffixes` | Identifiers indentados (>0 espaços/tabs) seguidos de `(`. Split case → suffix mais comum. Captura `*Async` em C#, `_*` em TS privates, etc. |

Schema enriquecido:
```json
{
  "suffix": "Service", "fileCount": 7, "folders": [...], "samples": [...],
  "namingPattern": "suffix-after",                              // NEW universal
  "declarationKeywords": ["public sealed", "public"],           // NEW universal
  "declarationSuffix": [": BaseService", ": BaseService, IFoo"],// NEW universal
  "topOfFileLines": ["using System.Threading.Tasks;", ...],     // NEW universal
  "memberSuffixes": ["Async"]                                   // NEW universal
}
```

**Por que é agnóstico de verdade**:
- Funciona pra .NET, TS, Python, Go, Rust, Dart, Java, Kotlin, PHP, Ruby, Swift sem código stack-specific
- Os VALORES capturados vêm 100% do código do user (nunca lista hardcoded)
- Lista mínima de comment-char prefixes (`//`, `#`, `--`, `/*`, `;`) é convenção universal de syntax, não tecnologia

**Limitações honestas**:
- Linguagens onde 1 file ≠ 1 declaração dominante (Lisp, Haskell modular): campos podem ficar vazios. Skill ainda gera com os 5 campos estruturais existentes. Não é regressão.
- Linguagens com comment chars exóticos (Erlang `%`): basta adicionar 1 entrada na lista de prefixes — ainda agnóstico de tecnologia.

**Fail-safe**: se qualquer field falha, retornar `null` no field — não quebrar o cluster.

### F2. Inline registry slice no agentPrompt
**Arquivo**: `.claude/scripts/scan/orchestrate.js` `renderPrompt()`

Em vez do agent fazer Read em `entity-registry.json`, o orchestrator filtra `_patterns[stack].discovered[]` pelo subproject e injeta **inline** no prompt:

```
## Clusters detected for this subproject

### Service (7 files)
- folders: src/Services, src/Modules/Auth/Services
- samples: UserService.cs, AuthService.cs, OrderService.cs
- commonBaseClass: BaseService
- namingConvention: {Entity}Service
- commonModifiers: public sealed
- asyncSuffix: 100%

### Repository (5 files)
...
```

**Custo**: ~200 chars × 10 clusters = 2k chars no prompt. Total prompt: ~8k chars (ainda sob budget de 20k).

### F3. Pre-extracted sample code no agentPrompt
**Arquivo**: `.claude/scripts/scan/orchestrate.js` `renderPrompt()`

Para cada cluster, ler 1 sample (top dos `samples[]`), extrair primeiras 60 linhas, injetar como bloco fence no prompt:

```
## Sample code per cluster

### Service — UserService.cs (lines 1-60)
\`\`\`csharp
[60 linhas verbatim]
\`\`\`
```

Agent não precisa Read essas — usa pra preencher `references/examples.md`. Custo: ~3k chars × 5 clusters = 15k chars adicionais (template fica ~25k chars). **Atenção**: passa do budget de 20k do `general-purpose`. Solução: subir budget desse role pra 30k em `context-budget.js` (era 5k tokens, vira 7.5k tokens) OU dispatch usando subagent_type customizado isento de cap.

### F4. Reescrever EVIDENCE RULE 3 (registry como fonte da verdade)
**Arquivo**: `.claude/scripts/scan/agent-prompt.template.md` + `templates/...` + `.claude/refs/scan/evidence-rules.md` + `templates/...`

Regra 3 nova:
> *Convention fields = fields do cluster object enriquecido (suffix, folders, fileCount, samples + os 5 campos novos universais quando presentes: namingPattern, declarationKeywords, declarationSuffix, topOfFileLines, memberSuffixes). NÃO faça Read adicional pra recalcular — o `cluster-discovery.js` já fez de forma agnóstica. Use os campos que o cluster tem (alguns podem vir nulos se a heurística não casou — apenas omita esses do skill). Adicione fields opcionais APENAS se você precisar de 1 Read em `cluster.samples[0]` (não 3).*

Regra 4 nova (examples.md):
> *`references/examples.md`: use o bloco "Sample code per cluster" injetado neste prompt. NÃO faça Read em samples — o código já está aqui. Copie verbatim.*

### F5. Refs externos como opt-in
**Arquivos**: `.claude/commands/mustard/scan-format/SKILL.md` + `templates/...`

Trocar `→ See ../../../refs/scan/*.md` por:
```
> Reference (optional, only if pattern is ambiguous): refs/scan/skill-generation.md
```
E adicionar no topo da scan-format:
> **Default**: agent prompt inline cobre 100% do protocolo. Refs em `refs/scan/` são detalhe opcional — não leia por safety.

Manter refs no disco (deletar é desperdício de conhecimento).

### F6. Path bug — fix definitivo
**Arquivos**: `.claude/refs/scan/skill-generation.md` (e templates) + `.claude/scripts/scan/orchestrate.js` + `agent-prompt.template.md` (e templates)

1. **Em `skill-generation.md`**: deletar seção "Subproject-Level Skills" (linhas 133-138). Manter apenas "Skills Location" (linhas 140-143).

2. **Em `orchestrate.js` `renderPrompt()`**: adicionar variável `{{absSubprojectPath}}` = `path.resolve(ROOT, sub.path)`.

3. **Em `agent-prompt.template.md`** step 6, deixar EXPLÍCITO:
   > *Skills go EXCLUSIVELY in `{{absSubprojectPath}}/.claude/skills/{skill-name}/SKILL.md`. The orchestrator runs from a different directory — never write to relative `.claude/skills/`. Path is absolute, no ambiguity.*

### F7. Self-budget soft (sem cap rígido)
**Arquivo**: `.claude/scripts/scan/agent-prompt.template.md` (e templates), antes de "Steps"

```
## Budget guidance (soft)
- Target: ~50 tool uses, ~30k tokens.
- Heurística: se últimos 3 Reads não revelaram pattern novo (mesma estrutura), PARE de explorar e produza skills com base no que tem.
- Skills incompletas > skills inventadas. Cite no return JSON: "tool_uses_used": N.
- O orchestrator já injetou clusters + sample code — uma skill típica precisa só de Glob (verificar paths) + Write (SKILL.md) + Write (examples.md). Estimativa: 2-3 ops por skill.
```

Não cap rígido — projetos grandes podem precisar mais. Apenas heurística qualitativa.

## Critical Files

- `C:\Atiz\Mustard\.claude\scripts\registry\cluster-discovery.js` (+ template) — F1
- `C:\Atiz\Mustard\.claude\scripts\scan\orchestrate.js` — F2, F3, F6
- `C:\Atiz\Mustard\.claude\scripts\scan\agent-prompt.template.md` (+ template) — F4, F5, F6, F7
- `C:\Atiz\Mustard\.claude\refs\scan\evidence-rules.md` (+ template) — F4
- `C:\Atiz\Mustard\.claude\refs\scan\skill-generation.md` (+ template) — F6 (deletar seção)
- `C:\Atiz\Mustard\.claude\commands\mustard\scan-format\SKILL.md` (+ template) — F5
- `C:\Atiz\Mustard\.claude\hooks\context-budget.js` — F3 (subir BUDGET_GENERAL de 20k pra 30k chars, ou criar role isento)

## Verification

1. **Sanity local**:
   ```
   node .claude/scripts/sync-registry.js --force
   ```
   Conferir que cluster JSON em `entity-registry.json` agora tem `namingConvention`, `commonModifiers`, `asyncSuffix`, `commonImports`.

2. **Prompt size**:
   ```
   node .claude/scripts/scan/orchestrate.js sialia-core --force
   ```
   Conferir `agentPrompt.length` < 30k chars (sob novo budget) e contém bloco "## Clusters detected" + "## Sample code per cluster".

3. **Dry run em sialia**:
   - Tool uses por agent: alvo ≤50 (era 42–109)
   - Tokens por agent: alvo ≤60k (era 87k–144k)
   - Mid-flight returns: alvo 0
   - Skills geradas em `sialia-{name}/.claude/skills/`, **nenhuma** em `Mustard/.claude/skillscore-*`
   - Skills devem conter Convention fields populados (não vazios)

4. **Qualidade downstream**: rodar `/mustard:feature` num CRUD trivial em sialia. Verificar se a skill gerada orienta o agent corretamente (naming, modifiers, async). Esse é o teste real de qualidade.

5. **Hook telemetry**: `node .claude/scripts/harness-views.js`. Confirmar que `context-budget` não bloqueou nenhum dispatch.

## Quality preserved (vs versão anterior do plano)

| Aspecto | Versão anterior | Versão atual |
|---|---|---|
| Convention fields | Cortados (só 5 do cluster) | Enriquecidos (5 estruturais + 5 universais derivados) |
| Sample code em examples.md | Cortado pra 1 Read opcional | Pre-injetado, agent só copia |
| Refs externos | Deletar | Manter como opt-in |
| Cap tool uses | Rígido (50) | Soft + heurística diminishing returns |
| Path bug | Texto explicativo | Variável `{{absSubprojectPath}}` |
| Agnosticismo do enrichment | Stack-specific (`_xxxFor(stackId)`) — falhava em Go/Rust/Dart | Heurísticas universais por arquivo (basename → declaration line) — funciona pra qualquer linguagem |

## Verificação de agnosticismo (checklist explícito)

Antes de aceitar implementação:
1. Buscar no diff do `cluster-discovery.js` por strings literais de keywords de stack: `class`, `def`, `func`, `fun`, `struct`, `using`, `import`, `package`, `Async`, `extends`, `implements`. Se aparecerem hardcoded em qualquer lugar do código novo → reprovar.
2. Apenas estruturas universais permitidas: split de case (`_splitPascalCase` já existente), regex de chars de comentário (lista mínima), indentação, posição relativa (linha 1-20), basename do arquivo.
3. Teste mental: "se uma stack inventada amanhã (X-lang) seguir convention 'arquivo = declaração principal' e usar `//` ou `#` ou `--` como comment char, o enrichment funcionaria sem mudar código?". Se sim → agnóstico. Se não → reprovar.
4. Os campos podem vir nulos pra alguma stack — isso é OK e esperado. NÃO é desculpa pra adicionar fallback stack-specific.

## Out of scope

- Não mexer em hooks (causa #5 do diagnóstico anterior — latência marginal).
- Não serializar agents (paralelo é UX/velocidade — usuário pediu rápido).
- Não adicionar hook novo de "scan-budget" — feedback de memory: subtrair, não adicionar.
- Cluster-discovery cache já existe — não tocar.
