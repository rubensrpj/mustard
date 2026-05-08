# Plano: auditoria do scan — cobertura agnóstica completa + performance

## Context

O usuário pediu para garantir que o `/mustard:scan` é **performático** e **sempre encontra os padrões necessários**, mantendo o princípio `feedback_mustard_agnostic`: nada de hardcode de tecnologia; tudo derivado do filesystem do usuário.

A auditoria desta sessão expôs gaps reais. O usuário rejeitou a primeira proposta de "deixar fora do escopo" decorators / hooks / cross-file / naming / outras stacks / paralelismo. Este plano os traz de volta com abordagem **agnóstica por sintaxe** — não importa lista de frameworks, só observa o que o código do usuário declara.

### Princípio do desenho agnóstico

Nenhuma das ações deste plano carrega nomes de tecnologia. Cada uma:
1. Detecta uma **construção sintática universal da linguagem** (regex sobre source code).
2. Os **valores** (nomes de classes, decoradores, funções) **emergem do código do usuário**, não de tabelas do Mustard.
3. Limiares são numéricos (≥3 ocorrências, etc.), não nominais.
4. Sem rede, sem catálogos externos.

Web-consult continua **vetada**: latência, não-determinismo, dependência de rede, qualidade variável da fonte, anti-padrão de "importar opinião pra dentro".

---

## Estado atual

### Cobertura existente

| Padrão | Estado | Arquivo |
|---|---|---|
| Suffix-cluster (PascalCase, cross-folder) | ✅ Sólido | `cluster-discovery.js:160-260` |
| Folder-cluster (mesma pasta) | ✅ Sólido | `cluster-discovery.js:107-143` |
| Base-class C# | ✅ Completo | `cluster-discovery.js:343-380` |
| Base-class TypeScript / Python | ❌ TODO `cluster-discovery.js:82` |
| Decorators / annotations | ❌ Não detectados |
| Function-prefix patterns (hooks, factories) | ❌ Não detectados |
| Import hubs (cross-file usage) | ❌ Não detectados |
| Naming convention dominante (filename) | ❌ Não detectado |
| Stacks dotnet/typescript/python/java/php/go/rust/dart | ✅ Scanner dedicado |
| Stacks ruby/c++/swift/kotlin standalone | ❌ Sem scanner |

### Performance

| Aspecto | Estado |
|---|---|
| `sync-detect.js` cache (TTL 5min, manifest hash) | ✅ Bom |
| `hashFileStream` (chunks 64KB) | ✅ Eficiente |
| `collectFiles` re-walk por stack | ⚠️ Sem cache cross-scanner |
| `cluster-discovery` sem cache de output | ⚠️ Re-runs recomputam tudo |
| Scanner execution serial | ⚠️ Sem `Promise.all` |
| Subset-prune O(K²) | ✅ K pequeno na prática |

---

## Ações detalhadas

### Tier 1 — Cobertura sintática direta (alta confiança, baixo risco)

#### Ação 1 — `extends`/herança em TypeScript e Python

**Por quê:** TODO explícito (`cluster-discovery.js:82`). Hoje C# detecta hierarquia, TS/Python não. Regex sintático puro.

**Arquivos:** `templates/scripts/registry/cluster-discovery.js` + mirror.

**Mudança:**
- `_discoverBaseClassClustersTypeScript`: regex `(?:export\s+)?(?:abstract\s+)?class\s+(\w+)\s+extends\s+(\w+)`. Filtrar generics. Limiar `MIN_BASE_CLASS_INHERITORS=3`.
- `_discoverBaseClassClustersPython`: regex `^class\s+(\w+)\s*\(\s*([\w.]+)`. Ignorar bases triviais (`object`, `Exception`, `BaseException`).
- Switch no `discoverClusters` por `stackId`.
- Remover TODO da linha 82.

**Agnóstico porque:** detecta sintaxe da linguagem (`extends`, `class X(Y)`); o nome de Y vem do código do usuário.

**Verificação:** fixture com 3+ classes TS extending `BaseService` → cluster gerado. Idem Python.

---

#### Ação 2 — Limites configuráveis com observabilidade

**Por quê:** `MAX_CLUSTERS=15`, `MIN_FILES_PER_SUFFIX=5`, `MIN_SUFFIX_LENGTH=6` cortam silenciosamente. Sufixos comuns (`Dto`, `Vm`) caem antes de chegar ao usuário.

**Arquivos:** `cluster-discovery.js:39-48`.

**Mudança:**
```js
const MIN_FILES_PER_SUFFIX = Math.max(2, parseInt(process.env.MUSTARD_CLUSTER_MIN_FILES, 10) || 5);
const MIN_SUFFIX_LENGTH    = Math.max(2, parseInt(process.env.MUSTARD_CLUSTER_MIN_SUFFIX_LEN, 10) || 6);
const MAX_CLUSTERS         = Math.max(1, parseInt(process.env.MUSTARD_CLUSTER_MAX, 10) || 15);
```

E **stderr log** dos clusters cortados acima de `MAX_CLUSTERS` (nome + fileCount), para o usuário saber que existem padrões além do top-N.

**Agnóstico porque:** só números configuráveis.

**Verificação:** test com 20 clusters + `MUSTARD_CLUSTER_MAX=10` → 10 retornam, stderr lista os 10 cortados.

---

#### Ação 3 — Skip-dirs configurável + leitura de `.gitignore`

**Por quê:** `DEFAULT_IGNORE` tem 13 nomes universais. Projetos reais têm `vendor/`, `Pods/`, `assets/`, `cdk.out/`, `coverage/`, `tmp/` — caminhados desnecessariamente. **A maioria já está em `.gitignore`** — usar isso como fonte agnóstica.

**Arquivos:** `templates/scripts/registry/file-utils.js:20-59`.

**Mudança:**
- Aceitar env `MUSTARD_SCAN_IGNORE` (lista CSV).
- **Ler `.gitignore` do subprojeto** (se existir), filtrar entradas que parecem nome de pasta (sem `/`, sem `*`, sem `!`), somar a `EXTRA_IGNORE`.
- Função pura `parseGitignoreDirs(content): string[]`.

**Agnóstico porque:** `.gitignore` é universal a Git, não de tecnologia.

**Verificação:** subprojeto com `.gitignore` listando `vendor/` → `collectFiles` ignora.

---

#### Ação 4 — Decorator/annotation cluster (universal sintático)

**Por quê:** `@Component`, `@Service`, `@Injectable`, `@Module`, `@Entity` (decoradores TS/Python/Java/Kotlin), `[ApiController]` (C# atributos), `#[Route]` (PHP 8+) — todos são padrões fortes no código real e hoje **invisíveis** ao scan.

**Agnóstico porque:** o Mustard não conhece nome nenhum. Detecta apenas a **sintaxe `@Word`** (ou `[Word]` em C#) imediatamente antes de `class`/`def`/`function` e **conta**. Os nomes que aparecerem (qualquer que sejam) emergem do código.

**Arquivos:** novo `_discoverDecoratorClusters` em `cluster-discovery.js`.

**Comportamento:**
- Por linguagem, regex sintático adequado:
  - TS/JS: `@(\w+)(?:\([^)]*\))?\s*\n?\s*(?:export\s+)?(?:abstract\s+)?(?:class|function)`
  - Python: `^@([\w.]+)(?:\([^)]*\))?\s*\n\s*(?:async\s+)?(?:class|def)`
  - Java/Kotlin: `@(\w+)(?:\([^)]*\))?\s+(?:public|private|internal|protected)?\s*(?:abstract\s+)?(?:class|fun)`
  - C#: `\[(\w+)(?:\([^)]*\))?\]\s*\n\s*(?:public|internal)?\s*(?:partial\s+)?class`
- Agrupar por nome do decorator, contar arquivos únicos.
- Limiar `MIN_DECORATOR_USAGE = 3` (configurável via env).
- Cluster type: `decorator-cluster`. Schema: `{kind: 'decorator-cluster', decorator: 'Component', fileCount: 12, samples: [...]}`.

**Verificação:** projeto Angular fictício com 5 `@Component` → cluster `decorator-cluster:Component`. Sem hardcode "Component".

---

#### Ação 5 — Function-prefix cluster (generaliza React hooks, factories, builders)

**Por quê:** padrões como `use*` (hooks), `make*` (factories), `with*` (HOCs), `is*`/`has*` (predicates) emergem em códigos reais. Hoje invisíveis ao scan.

**Agnóstico porque:** detecta **prefixo camelCase compartilhado por N+ funções top-level**. Não conhece "React" nem "hook". Se 12 funções começam com `use`, vira cluster `function-prefix-cluster:use`. Se 8 começam com `_internal`, idem.

**Arquivos:** novo `_discoverFunctionPrefixClusters` em `cluster-discovery.js`.

**Comportamento:**
- Por linguagem, regex de função top-level:
  - TS/JS: `(?:export\s+)?(?:async\s+)?function\s+([a-z]\w+)` + `(?:export\s+)?const\s+([a-z]\w+)\s*=\s*(?:async\s*)?(?:\([^)]*\)|[a-z])\s*=>` (arrow funcs)
  - Python: `^def\s+([a-z_]\w+)` (top-level apenas — ignorar indented)
- Extrair "prefixo" via split camelCase/snake_case: primeiras N letras minúsculas até primeira maiúscula ou underscore.
- Agrupar por prefixo, limiar `MIN_FUNCTION_PREFIX_USAGE = 5`.
- Limiar de comprimento mínimo do prefixo: 2 caracteres (descartar `f`, `a`).
- Cluster type: `function-prefix-cluster`.

**Agnóstico porque:** prefixos vêm das funções do usuário; Mustard não conhece nenhum.

**Verificação:** fixture TS com 12 `useFoo`, `useBar`, … → cluster `function-prefix-cluster:use`.

---

#### Ação 6 — Naming-convention dominante por filename

**Por quê:** projetos misturam `kebab-case.ts`, `camelCase.ts`, `PascalCase.ts`, `snake_case.py`. Saber **a convenção dominante** orienta agentes downstream a respeitar o padrão na criação de novos arquivos.

**Agnóstico porque:** classifica arquivos via regex e conta. Não há tabela "qual stack usa qual convenção".

**Arquivos:** novo `templates/scripts/registry/project-conventions.js` (criado também em Ação 9 — fundir).

**Comportamento:**
- Para cada arquivo (de extensão primária do stack), classificar nome:
  - `^[a-z][a-z0-9]*(-[a-z0-9]+)*$` → `kebab-case`
  - `^[a-z][a-zA-Z0-9]*$` → `camelCase`
  - `^[A-Z][a-zA-Z0-9]*$` → `PascalCase`
  - `^[a-z][a-z0-9_]*$` → `snake_case`
  - resto → `mixed`
- Reportar percentuais. Convenção dominante = ≥60% dos arquivos.
- Inserir em `entity-registry.json._conventions.naming = {dominant, distribution}`.

**Verificação:** fixture com 80% `PascalCase.cs`, 20% outros → `dominant = PascalCase`.

---

#### Ação 7 — Cluster-cache por hash de file-set

**Por quê:** rodar `/scan` 2x em projeto inalterado recomputa tudo. Performance.

**Arquivos:** `cluster-discovery.js` + `.claude/.cluster-cache.json`.

**Comportamento:** hash determinístico de `(stackId, allFiles ordenados, mtime de cada)`. Cache hit → retorna `cached.clusters`. Miss → recomputa e grava.

**Agnóstico porque:** infraestrutura, não tecnologia.

**Ganho:** 2ª execução ~10x mais rápida na fase de cluster.

**Verificação:** time 1ª vs 2ª execução; tocar mtime invalida.

---

### Tier 2 — Cobertura ampliada (escopos maiores, mas viáveis agnosticamente)

#### Ação 8 — Project-conventions reader (configs declarados pelo usuário)

**Por quê:** complementar à Ação 6. O projeto declara em arquivos do próprio repo várias convenções: indent, line length, paths, dependências, ignores. O Mustard só **lê o que o projeto já declarou**, sem importar nada externo.

**Arquivos:** `templates/scripts/registry/project-conventions.js` (mesmo arquivo da Ação 6).

**Fontes (todas opcionais, fail-silent):**
- `.editorconfig` → `indent_style`, `indent_size`, `end_of_line`.
- `tsconfig.json` (se TS) → `strict`, `paths`, `target`.
- `package.json` (se JS/TS) → scripts (test, build), dependencies (sinal de stack).
- `pyproject.toml` (se Python) → `[tool.ruff]`, `[tool.black]`, `[tool.pytest]`, `[project]`.
- `Cargo.toml` (Rust) → `[workspace]`, `[lib]`.
- `go.mod` → module path.
- `composer.json` (PHP) → autoload paths, dependencies.

Saída: `entity-registry.json._conventions = {indent, naming, frameworks, paths, ...}`.

**Agnóstico porque:** lê o que o projeto declarou. Zero hardcoded.

**Verificação:** subprojeto Python com `pyproject.toml [tool.ruff] line-length = 100` → `_conventions.lineLength = 100`.

---

#### Ação 9 — Import-hub cluster (cross-file usage frequency)

**Por quê:** entender "quem é importado por muitos" é forte sinal de domínio (utility hubs, central abstractions).

**Agnóstico porque:** parsing de import é sintaxe universal por linguagem; não conhece nomes específicos.

**Arquivos:** novo `_discoverImportHubClusters` em `cluster-discovery.js` + cache dedicado.

**Comportamento:**
- Por linguagem, regex de import:
  - TS/JS: `import\s+(?:[^'"]+from\s+)?['"]([^'"]+)['"]` + `require\(['"]([^'"]+)['"]\)`
  - Python: `^from\s+([\w.]+)\s+import` + `^import\s+([\w.]+)`
  - Java: `^import\s+([\w.]+);`
  - C#: `^using\s+([\w.]+);`
  - Go: `^\s*"([^"]+)"` (dentro de bloco import)
  - Rust: `^use\s+([\w:]+)`
- Resolver imports relativos para path absoluto.
- Construir grafo `importee → [importers]`.
- Identificar **hubs**: importees com `importers.length >= MIN_HUB_IMPORTERS=8` (configurável).
- Filtrar imports externos (node_modules, stdlib): só cross-internal.
- Cluster type: `import-hub-cluster`. Schema: `{kind, hub: 'src/utils/logger.ts', importedByCount: 23, samples: [importer paths]}`.

**Performance:** parsing é caro; cache obrigatório por mtime de cada arquivo. `Promise.all` over reads.

**Agnóstico porque:** sintaxe de import por linguagem; nomes vêm do código.

**Verificação:** fixture com 10 arquivos importando `./utils/logger` → cluster gerado.

---

#### Ação 10 — Scanners para stacks restantes (Ruby, C/C++, Swift, Kotlin standalone)

**Por quê:** hoje 4 linguagens populares caem em fallback genérico (cluster-discovery em cima de extensão), perdendo análise semântica.

**Agnóstico porque:** cada scanner detecta sintaxe da própria linguagem (regex/parsing simples), não convenção de framework.

**Arquivos novos:**
- `templates/scripts/registry/scanners/ruby-scanner.js` — signal `Gemfile`/`*.gemspec`/`config.ru`; ext `.rb`.
- `templates/scripts/registry/scanners/cpp-scanner.js` — signal `CMakeLists.txt`/`Makefile`/`*.vcxproj`; ext `.cpp`/`.h`/`.hpp`.
- `templates/scripts/registry/scanners/swift-scanner.js` — signal `Package.swift`/`*.xcodeproj`; ext `.swift`.
- `templates/scripts/registry/scanners/kotlin-scanner.js` — signal `build.gradle.kts` (sem `pom.xml`); ext `.kt`.

Cada scanner segue `scanner-contract.js` (interface comum: `detect()`, `scan()`).

Adicionar `STACK_SIGNALS` em `scanner-loader.js`. Atualizar testes.

**Verificação:** subprojeto Ruby com Gemfile + `*.rb` → stack detectado, scanner roda, clusters emitidos.

---

#### Ação 11 — Paralelizar scanners e reads via `fs.promises` + `Promise.all`

**Por quê:** hoje cada scanner roda serial e usa `readFileSync`. Em monorepo multi-stack (.NET + TS + Python), tempo total = soma dos tempos.

**Estratégia:**
- Converter `collectFiles` para `collectFilesAsync` usando `fs.promises.readdir`/`readFile`. Manter wrapper sync para compatibilidade.
- `sync-registry.js` invoca scanners via `await Promise.all(scanners.map(s => s.scan()))`.
- Cluster-discovery interno também: leitura de arquivo para `_discoverBaseClassClusters` em paralelo via `Promise.all`.

**Cuidado:** Node.js fs sync usa thread pool; converter para promises libera event loop e permite interleaving real do disco. Ganho prático: 30-50% em multi-stack.

**Risco:** mudança de contract dos scanners (sync → async). Tests precisam adaptar.

**Verificação:** benchmark sintético — 3 scanners em paralelo vs serial. `time` mostra redução.

---

## Princípios e garantias agnósticas

| Ação | Hardcoded de tecnologia? | Justificativa |
|---|---|---|
| 1 — extends TS/Python | Não | Sintaxe da linguagem; Y emerge do código |
| 2 — limites configuráveis | Não | Apenas números |
| 3 — skip-dirs + .gitignore | Não | `.gitignore` é Git, não tech |
| 4 — decorator-cluster | **Não** | Regex `@Word`; nomes emergem |
| 5 — function-prefix-cluster | **Não** | Prefixo de função; emerge do código |
| 6 — naming-convention dominante | **Não** | Classifica por regex; conta |
| 7 — cluster-cache | Não | Hash |
| 8 — project-conventions reader | **Não** | Lê configs do próprio projeto |
| 9 — import-hub-cluster | **Não** | Sintaxe de import; hubs emergem |
| 10 — scanners restantes | **Não** | Sintaxe de cada linguagem |
| 11 — paralelizar | Não | Infraestrutura |

**Web-consult continua vetada.** A Ação 8 substitui-a no agnóstico real: lê o que o projeto declarou.

---

## Ordem de execução sugerida

**Tier 1 (baseline robusto, ~10h):**
1. **Ação 2** — limites configuráveis (~1h, ganho de observabilidade imediato)
2. **Ação 3** — skip-dirs + .gitignore (~1h)
3. **Ação 1** — extends TS/Python (~2h, fecha TODO existente)
4. **Ação 4** — decorator-cluster (~2h, alta cobertura por baixo custo)
5. **Ação 5** — function-prefix-cluster (~1.5h)
6. **Ação 6** — naming dominante (~1h)
7. **Ação 7** — cluster-cache (~2h, performance)

**Tier 2 (extensão, ~12h):**
8. **Ação 8** — project-conventions reader (~2h)
9. **Ação 9** — import-hub-cluster (~4h, requer cache cuidadoso)
10. **Ação 10** — scanners restantes (~3h, 4 stacks × ~45min)
11. **Ação 11** — paralelizar (~3h, refactor de contract)

**Total estimado:** ~22h de implementação + ~6h de testes.

Cada ação é independente. Pode parar em qualquer ponto. Tier 1 sozinho já fecha os gaps mais visíveis e mantém performance.

---

## Arquivos críticos

| Arquivo | Ações | Tipo |
|---|---|---|
| `templates/scripts/registry/cluster-discovery.js` | 1, 2, 4, 5, 7, 9 | edit grande |
| `.claude/scripts/registry/cluster-discovery.js` | idem | edit (mirror) |
| `templates/scripts/registry/file-utils.js` | 3, 11 | edit |
| `.claude/scripts/registry/file-utils.js` | idem | edit (mirror) |
| `templates/scripts/registry/project-conventions.js` | 6, 8 | new |
| `.claude/scripts/registry/project-conventions.js` | idem | new (mirror) |
| `templates/scripts/registry/scanners/{ruby,cpp,swift,kotlin}-scanner.js` | 10 | new (4 arquivos) |
| `templates/scripts/registry/scanner-loader.js` | 10, 11 | edit |
| `templates/scripts/registry/scanner-contract.js` | 11 | edit (sync→async) |
| `templates/scripts/registry/schema-builder.js` | 6, 8, 9 | edit (consume `_conventions`, hubs) |
| `templates/scripts/sync-registry.js` | 11 | edit (Promise.all) |
| `templates/scripts/registry/__tests__/*` | todas | new tests |
| `templates/CLAUDE.md` | 2, 3 | doc env vars |

---

## Verificação end-to-end

```bash
# Tier 1
node --test templates/scripts/registry/__tests__/

# Cobertura nova: decoradores + function prefixes
# (rodar em projeto teste com Angular ou React)
node templates/scripts/sync-registry.js --force
cat .claude/entity-registry.json | grep -E "decorator-cluster|function-prefix-cluster"

# Limites configuráveis: drops vão para stderr
MUSTARD_CLUSTER_MAX=3 node templates/scripts/sync-registry.js --force 2>&1 | grep dropped

# Skip-dirs custom + .gitignore
MUSTARD_SCAN_IGNORE=Pods node templates/scripts/sync-registry.js --force

# Cache hit (Ação 7)
node templates/scripts/sync-registry.js --force      # cold
time node templates/scripts/sync-registry.js --force  # warm
# expect: warm < 50% do cold

# Tier 2 (após implementar)
# Convenções declaradas
cat .claude/entity-registry.json | jq ._conventions

# Import hubs
cat .claude/entity-registry.json | grep import-hub-cluster

# Stacks restantes (Ruby/C++/Swift/Kotlin)
node templates/scripts/sync-registry.js --force  # em projeto Ruby
# expect: scanner ruby roda, clusters emitidos

# Paralelismo (benchmark)
time node templates/scripts/sync-registry.js --force  # em monorepo multi-stack
# expect: ~30-50% mais rápido vs baseline
```

---

## Riscos e mitigações

| Risco | Mitigação |
|---|---|
| Regex de decorators/função/imports falha em sintaxe edge | Tests com fixtures reais; fail-open silencioso |
| Limiares novos (MIN_DECORATOR_USAGE etc.) muito permissivos → ruído | `Math.max(2, ...)` floor; configurável |
| Cluster cache invalidação errada (mtime FS-dependent) | Hash inclui `(filename, size, mtime)`; corrupção do cache → recomputa |
| Async refactor (Ação 11) quebra scanners existentes | Mantém wrappers sync por compat; tests cobrem ambos |
| Import-hub graph custoso em monorepo de 50k arquivos | Cache obrigatório; limiar de hub alto evita ruído |
| Scanners novos (Ruby/C++/Swift/Kotlin) introduzem bugs específicos | Cada scanner com test isolado; fallback genérico se scanner falhar |

---

## Impacto na orquestração de skills

A geração e o consumo de skills seguem mecânica documentada em `templates/refs/scan/skill-generation.md`:

```
sintaxe do código
   ↓ (scanners + cluster-discovery)
entity-registry.json._patterns[stackId].discovered[]
   ↓ (Task agents do scan; regra "skill-creator methodology")
SKILL.md por padrão (validados por skill-validate-gate, já ativo)
   ↓ (Claude Code runtime, description-based auto-loading)
agente despachado consome só os relevantes
```

### Impacto direto deste plano

| Dimensão | Antes | Depois Tier 1 | Depois Tier 2 |
|---|---|---|---|
| Cluster types disponíveis | suffix, folder, base-class (C# apenas) | + extends (TS/Python), decorator, function-prefix | + import-hub |
| Skills emitidos por subprojeto | 0–3 | 5–10 | 8–15 |
| Cobertura de stacks | 8 | 8 (com gaps fechados em TS/Python) | 12 |
| `## Convention` em SKILL.md inclui declarado pelo projeto | ❌ | ✅ (Ação 6) | ✅ (Ação 8 amplia) |
| Re-execução de scan | Recomputa tudo | 2× mais rápido (Ação 7 cache) | 30-50% mais rápido (Ação 11 paralelismo) |

### Como a orquestração não é afetada negativamente

A seleção de skills no runtime continua sendo decisão do Claude Code (`description` matching), não do Mustard. Garantias preservadas:

- **MAX_CLUSTERS=15** (Ação 2) limita explosão por subprojeto.
- **skill-validate-gate** (já implementado) reprova SKILL.md sem trigger words ou mal-formado.
- **skill-size-gate** mantém SKILL.md ≤500 linhas.
- **`agent-judgment filter`** (`skill-generation.md:45`) descarta clusters com <3 arquivos ou nomes genéricos.
- **Naming `{subproject-prefix}-{concept}`** evita colisão entre subprojetos.
- **Budget de Task prompt** (`context-budget.js`) continua protegendo agents.
- **`recommended-skills-audit.js`** loga quando o dispatch lista >10 skills — observabilidade pra ajustar limites.

### Caso real: pipeline `/mustard:feature add-user-controller` (NestJS hipotético)

**Hoje:** agent recebe 1 skill (`backend-service-pattern` via suffix cluster). Convenção: `folder: src/services`, naming `PascalCase`. Decorator `@Injectable` invisível ao registro. Agent escreve service sem decorator (gap real).

**Depois Tier 1:** agent recebe 3-4 skills auto-loaded:
- `backend-service-pattern` (mantido)
- `backend-injectable-decorator-pattern` (NOVO via Ação 4)
- `backend-controller-pattern` (já existia)
- `_conventions.indent_size=2` (NOVO via Ação 6) embutido em todas

Agent escreve service com `@Injectable`, indent correto, herdando se houver `BaseService` (Ação 1).

**Depois Tier 2:** agent também recebe `backend-logger-utility` (Ação 9) com path do hub. Reusa importação existente em vez de recriar logger.

### Risco residual e circuit breakers

| Risco | Circuit breaker |
|---|---|
| Skills demais carregados pelo runtime → contexto inchado | `recommended-skills-audit.js` warn em >10; usuário ajusta `MUSTARD_CLUSTER_MAX` |
| Trigger overlap (2 skills disputam mesma task) | Description "pushy mas específica" mandatória (validate-gate); regra existente em `skill-generation.md:118` |
| Convenção declarada conflita com cluster detectado | Cluster vence (sintaxe observada > config); documentar em `## Rules` da SKILL.md |
| Skills do scan ficam stale após code change | `cluster-cache` da Ação 7 invalida por mtime; re-scan é incremental |

---

## Garantia explícita: `/mustard:scan` não regride

- Continua lendo `entity-registry.json` como fonte autoritativa.
- Todas as ações **adicionam** clusters/sinais; nenhuma remove existente.
- Comportamento atual preservado se todas as env vars estiverem ausentes.
- Schema `entity-registry.json` é aditivo (`_conventions` opcional, novos cluster types coexistem).

---

## Não inclui (genuinamente fora do escopo)

- **Web-consult** — vetada por latência, não-determinismo, dependência de rede, qualidade variável da fonte. Substituída por Ação 8 (configs declarados pelo usuário).
- **Inferência de naming convention sem `.editorconfig` quando o filename não dá sinal** (ex: nomes mistos): Ação 6 reporta como `mixed`; não inventa.
- **Análise semântica framework-specific** (entender "isso é uma rota Express") — exigiria importar conhecimento de framework. Anti-agnóstico.
- **Detecção de design patterns clássicos** (Singleton, Factory, Strategy por estrutura de classe): heurística com falsos positivos altíssimos.
- **Type-graph (TS/Java) para inferir abstrações**: requer compilador completo; fora do escopo de scan ágil.
