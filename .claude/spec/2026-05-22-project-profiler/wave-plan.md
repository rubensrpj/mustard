# Wave plan — project-profiler

## Visão geral

Cinco waves funcionais em sequência, da performance pura (sem modelo) até o grafo de injeção e o write-back. Cada wave depende da anterior compilar e passar nos próprios ACs. Performance primeiro: a W1 entrega valor (scan rápido) antes de qualquer modelo entrar em cena.

```
W1 (rt) ─► W2 (rt) ─► W3 (rt+templates) ─► W4 (rt) ─► W5 (rt+templates) ─► REVIEW ─► QA
perf       interpret   grafo+vault          resolver    write-back
```

| # | Slug | Role | Modelo | Resumo |
|---|---|---|---|---|
| 1 | `wave-1-rt` | rt | opus | Passada única + paralela (rayon): colapsa as ~6 varreduras numa só, reusa ignore-set/.gitignore, teste de paridade + benchmark. Sem LLM. |
| 2 | `wave-2-rt` | rt | opus | Camada de interpretação (Sonnet, cold path, cacheada por SHA): rotula clusters, resolve `dominant:null`, identifica entidades, escreve as arestas `[[ ]]`. Elimina os 8 `*_scanner.rs`. |
| 3 | `wave-3-rt` | rt + templates | opus | Schema de nó-conceito + convenção `[[id]]`; vault em `.claude/` (`graph/`, `.obsidian/`, `index.md` MOC, `aliases` no `SKILL.md`); índice `id→path` no sync; validação de arestas (órfã/ciclo). |
| 4 | `wave-4-rt` | rt | opus | `context-resolve`: BFS no grafo → fecho mínimo, corte por budget, dereferência `id→path`. Unifica os 4 mecanismos de carga (skill-match, recipe-match, refs, context-slice). |
| 5 | `wave-5-rt` | rt + templates | opus | Pipeline grava `spec → [[skill]]` de volta (aresta `injected` vs `applied`); MOC e backlinks; AC de redução de token via telemetria `budget`. |

## Dependências entre waves

- **W2** depende de W1 (consome o perfil compacto produzido pela passada única).
- **W3** depende de W2 (os nós e arestas nascem da interpretação).
- **W4** depende de W3 (o resolver anda no grafo e usa o índice `id→path`).
- **W5** depende de W4 (write-back registra o fecho que o resolver injetou) e da telemetria do `budget` já existente.

Sequencial por convenção do Mustard (gates simples). Sem paralelismo entre waves.

## Roles e agentes alvo

- `rt` → `rt-impl` (acesso a `apps/rt/`)
- `templates` → `cli-impl` (acesso a `apps/cli/templates/`, payload copiado para o `.claude/` alvo)

## Gates entre waves

Cada wave só avança quando: (1) `cargo build --workspace` passa; (2) `cargo clippy -p mustard-rt -- -D warnings` passa; (3) `cargo test -p mustard-rt` passa; (4) ACs do `wave-N-*/spec.md` retornam pass via `mustard-rt run qa-run --spec ...`. Falha em qualquer gate → não inicia a próxima; sub-spec tactical-fix se necessário.

## Fronteira conhecimento × plumbing (regra do grafo)

Vira `[[wirelink]]` (entra no grafo): convenção→convenção, skill→recipe/convenção, entidade→padrão, spec→skill, command→ref progressivo.

Continua path (fora do grafo): `CLAUDE.md`→`pipeline-config.md`, `settings.json`→hooks, imports de código. Arrastar plumbing pro grafo polui o graph view e faz o resolver seguir arestas que não são relevância — proibido.

## Layout do vault (cravado na W3)

```
.claude/                  ← vault root (abre no Obsidian)
├── .obsidian/            ← config do app (sem links)
├── graph/
│   ├── index.md          ← MOC (porta de entrada, gerado)
│   ├── {sub}.conv.*.md   ← nós-conceito, ids únicos, arestas [[ ]]
│   └── {sub}.entity.*.md
├── skills/<n>/SKILL.md   ← pesado; frontmatter aliases:[{sub}.skill.<n>]
└── entity-registry.json  ← faceta entidade (gerada do mesmo perfil)
```

`id` = `{sub}.{kind}.{slug}` (kebab). É o nome navegável no Obsidian e a chave do índice `id→path` que o resolver dereferencia. O Claude Code nunca vê `[[ ]]` cru — recebe o conteúdo resolvido.

## Riscos por wave e mitigação

| Wave | Risco | Mitigação |
|---|---|---|
| W1 | Paralelismo introduzir não-determinismo na ordem do registry | Saída ordenada (`BTreeMap` já é; coletar paralelo, reduzir ordenado); AC de paridade byte-estável é obrigatório |
| W2 | Interpretação do modelo escrever aresta errada | Cold path + cache congelado por SHA; `cluster_discovery` agnóstico é o piso; aresta é JSON/markdown editável; modelo via env (default sonnet) para medir |
| W2 | Custo de modelo a cada sync | Roda só no 1º sync ou quando file-set muda além do threshold (mecanismo do `.cluster-cache.json`); congelado |
| W3 | Colisão de nome no Obsidian (`SKILL.md`, `guards.md`) | `id` namespaceado único + `aliases` no frontmatter; graph view limpo |
| W4 | Resolver virar heurística complexa | Só BFS determinístico sobre arestas explícitas; fail-open; cacheado por hash do escopo |
| W5 | Aresta `applied` ser inferência vendida como fato | Distinguir explícito `injected` (certo) vs `applied` (inferido); telemetria do `budget` valida relevância |

## ACs transversais

Definidos no `spec.md` parent (AC-P-1..8). Cada wave detalha os seus.
