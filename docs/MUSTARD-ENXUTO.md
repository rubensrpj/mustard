# MUSTARD ENXUTO — reposicionamento pós-superpowers

> Análise prospectiva (2026-07-07). Origem: comparação com o [superpowers](https://github.com/obra/superpowers)
> (framework de skills do Jesse Vincent, aceito no marketplace oficial da Anthropic, multiplataforma).
> Este documento é o plano permanente; a execução acontece por fases aprovadas uma a uma.

## 1. A decisão em uma frase

O mustard deixa de competir como **metodologia completa** e se concentra no que só ele tem —
**travas que garantem** (hooks que bloqueiam de verdade), **conhecimento do projeto** (scan → Guards)
e **medição** (eventos → dashboard) — enquanto a **metodologia** (brainstorm, plano, TDD, review)
vive em skills leves de markdown, escritas no padrão que comprovadamente funciona.

## 2. Por que agora — três fatos

1. **O superpowers comoditizou a metade "metodologia".** Ele entrega brainstorm → plano → TDD →
   subagentes → review → verificação com ~14 skills de markdown, custo de manutenção ≈ zero,
   instalação em 2 minutos, mantido de graça pela comunidade. Reimplementar isso em Rust é pagar
   caro pelo que existe pronto.
2. **O superpowers NÃO tem — e estruturalmente não terá — o que o mustard tem.** A disciplina dele
   é *persuasão* (texto bem escrito que o modelo pode ignorar sob pressão); a do mustard é *trava*
   (o close-gate recusa fechar sem QA aprovado, o modelo querendo ou não). Ele também não conhece
   o repositório do usuário (metodologia genérica; os Guards do mustard sabem as regras de cada
   camada) e não enxerga nada (zero telemetria; o mustard tem painel).
3. **A medição de campo do próprio mustard aponta o mesmo corte.** As avaliações registradas dizem:
   o valor entregue é a disciplina (AC executáveis, gates) e os Guards por camada; a camada
   semântica cara (recall/embeddings/juízes) quase não dispara em uso real — busca literal resolvia.

## 3. Arquitetura-alvo: professor × prova

Todo momento do fluxo tem dois lados: o **professor** (a skill que ensina o jeito certo — barato,
markdown) e a **prova** (a trava que impede o jeito errado — Rust, determinístico). O superpowers
só tem professores; o alvo do mustard é ter os dois, com o professor no padrão de escrita deles e
a prova no motor que já existe:

| Momento | Professor (skill/markdown) | Prova (trava determinística) |
|---|---|---|
| Entender antes de codar | grill inline no /feature | `scope_guard` + spec com `meta.json` |
| Planejar | wave-plan (writing-plans) | `plan-materialize` + aprovação com plano visível |
| Isolar o trabalho | — (nem precisa ensinar) | `work_branch_gate` cria `{base}_{slug}` na 1ª edição |
| Executar via subagentes | contratos por papel (`--task-text`) | `subagent_inject` + caps de retorno |
| Testar | AC como comandos executáveis | `qa-run` roda cada AC e grava `qa.result` |
| Revisar | agente `mustard-review` (read-only) | fase REVIEW exigida no fluxo Full |
| Fechar | verification-before-completion | `close-gate` bloqueia sem QA pass + STALE re-verify |
| Registrar | — | `emit-pipeline` + eventos NDJSON → dashboard |

## 4. Mapa do código com veredito por módulo

Estado atual: **414 arquivos Rust em 8 crates** + dashboard Tauri (195 arquivos TS/TSX).
Vereditos: **FICA** (núcleo), **OBSERVA** (congela investimento e mede uso), **CORTA** (remoção
de baixo risco), **AUDITAR** (manter o que o painel consome, cortar o resto).

### FICA — o núcleo de garantia e conhecimento
- `apps/rt/src/hooks/*` (bash, session, task, observe, write) — as travas em si.
- `apps/rt/src/commands/event/` — trilha de eventos (emit, phase, projections, verify).
- `apps/rt/src/commands/pipeline/` — composições determinísticas que os gates usam
  (close_pipeline, plan_materialize, dispatch_plan, resume_bootstrap).
- `apps/rt/src/commands/{wave,spec,checklist,scan_guards,review,agent,maint,doctor,statusline}/`.
- `apps/scan` + parte scan/grain de `packages/core` — o mapa do projeto (grain.model.json,
  Guards, render do CLAUDE.md).
- `apps/cli` — instalação/templates. `apps/dashboard` — o painel. Eventos NDJSON.

### OBSERVA — congelado até a Fase 0 decidir
- `apps/embed` — busca por significado (embeddings) e toda a cadeia de vetores que ela puxa.
- `apps/rt/src/commands/knowledge/` — recall, memory, ingest, prune (memória semântica).
- `apps/rt/src/commands/i18n/` — tradutor do digest (só existe por causa da cadeia semântica).
- `digest_precision`, juízes (concern-judge, digest-validate) e a parte de léxico/enrich do core.

### CORTA — baixo risco, independe de medição
- `apps/rt/src/commands/capability/` — extração OpenSpec construída e nunca implantada.
- `apps/rt/src/commands/migrate/` — migrações pontuais já executadas (spec_headers, to_meta).
- `apps/rt/src/commands/economy/otel/` — ponte OTEL; o painel lê NDJSON diretamente.

### AUDITAR — cortar o que o painel não consome
- `apps/rt/src/commands/economy/` (rtk_gain, token_budget, context_budget, baseline/reconcile/
  report, transcript_watcher) — manter apenas as consultas que alimentam telas reais do dashboard.

## 5. Fases — o que será feito e o benefício de cada uma

### Fase 0 — Medir o uso real da camada semântica (1 sessão · zero risco · só leitura)
**O que:** extrair dos eventos NDJSON (mustard + sialia): % de execuções que chamaram
digest/recall; % em que o arquivo de fato aberto veio do resultado semântico e não estaria num
grep óbvio ("mudou o resultado"); custo por chamada da cadeia enrich + juízes. O loop de outcome
(keystone, já corrigido) fornece o sinal.
**Benefício:** a decisão de corte da Fase 3 sai de dado, não de opinião.
**Critério pré-combinado:** "mudou o resultado" < 10% em 2–3 semanas de uso real → corta;
≥ 25% → mantém e melhora; entre os dois → permanece congelado.

### Fase 1 — Skills no padrão que funciona (1–2 sessões · só markdown/templates)
**O que:** reescrever as skills de fluxo (feature, bugfix, task, qa, close, git, grill) com as
quatro técnicas comprovadas do superpowers:
1. **Lei de ferro** no topo (uma frase absoluta: "NENHUM CLOSE SEM QA PASS");
2. **Tabela anti-racionalização** (desculpa provável → resposta pronta);
3. **Red flags** (frases que o agente diz quando está prestes a furar o processo);
4. **Disclosure progressivo** (SKILL.md curto; detalhe em `refs/` carregado sob demanda —
   o mustard já tem `refs/`, isto formaliza o padrão).
Validar com **pressure-test**: cenários simulados de pressão ("produção caiu, pula o QA?") como
avaliação de obediência das skills.
**Benefício:** mais obediência sem uma linha de Rust nova; a melhoria viaja no template para todo
projeto; elimina a única vantagem de conteúdo do superpowers mantendo a trava como piso.

### Fase 2 — Interruptor da camada semântica (1 sessão)
**O que:** flag em `mustard.json` (`"semantic": "off" | "shadow" | "on"`). Em `shadow`, a camada
roda e registra o que TERIA retornado, sem injetar no contexto — alimenta a Fase 0 sem enviesar o
uso. Nenhum investimento novo na camada até a decisão.
**Benefício:** para de pagar manutenção na parte não comprovada; reversível com uma linha.

### Fase 3 — Corte guiado pelos números (1–2 sessões · mediante dado da Fase 0)
**O que:** se o critério de corte bater: remover `apps/embed`, `knowledge/recall*`, `i18n/`,
juízes, `digest_precision` e a parte semântica do core; localização passa a grep/glob + Guards.
Independente do dado (já aprovável): `capability/`, `migrate/`, `otel/`.
**Benefício:** menos superfície = reinstalação mais simples (reduz o ciclo "corrigido mas não
implantado" recorrente no histórico), build mais rápido, menos lugar para bug morar.

### Fase 4 — Convivência com o superpowers num projeto-teste (opcional · 1 sessão)
**O que:** instalar o superpowers no sialia AO LADO do mustard; verificar que as skills dele
(metodologia) e as travas do mustard (garantia) convivem; decidir o que importar como base de
conteúdo (licença MIT permite) versus manter próprio.
**Benefício:** metodologia mantida de graça pela comunidade; o esforço do mustard concentra-se
100% no que só ele oferece.

## 6. Riscos e reversibilidade

- **Falso negativo na Fase 0** (a camada semântica valeria em projetos maiores que os medidos):
  mitigado pelo modo `shadow` — ela continua registrando o que teria acertado, sem custar contexto.
- **Cortar algo que o painel usa:** a Fase 3 só remove após o AUDITAR mapear consumo real das
  telas; qualquer remoção é um commit isolado e reversível.
- **Skills mais duras irritarem no dia a dia:** o pressure-test da Fase 1 mede obediência E
  fricção; leis de ferro só nos pontos que já têm trava (onde a skill apenas explica o inevitável).

## 7. O que este plano NÃO é

- Não é adotar o superpowers como dependência — é adotar as *técnicas de escrita* dele e,
  opcionalmente (Fase 4), conviver com ele.
- Não é abandonar a camada semântica por opinião — é submetê-la ao mesmo padrão de prova que o
  mustard exige de todo mundo: evidência de campo antes de fechar.

---

## 8. EXECUTADO — 2026-07-07 (branch `dev_mustard-enxuto`)

Decisão do dono (uso pessoal, único usuário): **sem período de medição** — as avaliações de campo
já registradas bastaram como evidência. Fases 0/2 dispensadas; 1 e 3 executadas de uma vez, com
duas auditorias read-only na frente (consumo do dashboard + mapa de acoplamento) no lugar da
medição. Leis do trabalho: agnóstico + dashboard intacto.

**Correções que as auditorias impuseram ao plano original (§4):**
- `capability/` NÃO era morto (spec-lint reusa o parser; doctor roda capability_drift_check;
  agent_prompt_render injeta capabilities) — **mantido**.
- `economy/otel/` NÃO era duplicado (o collector ESCREVE `pipeline.telemetry.*` que o painel
  renderiza; session hooks o spawnam/derrubam) — **mantido inteiro**, junto de transcript_watcher
  e rtk_gain.
- `knowledge/memory.rs` **mantido** (painel chama `run memory search`; agent_summary_observer
  persiste memória) — só recall/recall_cli/prune/memory_ingest saíram.
- `commands/i18n/` era só translate-heading (não "o tradutor do digest") — saiu; o i18n real
  (`core platform/i18n`) ficou.
- O digest determinístico (`mustard-rt run feature`, BM25) **ficou** — spec-draft, scope-classify
  e glossary dependem dele. O que saiu foi o LLM-sobre-LLM: embed/vetores, juízes, recall.

**Corte Rust:** 30 arquivos + 3 dirs deletados (apps/embed inteiro, benchmarks/, digest_precision,
recall_bench, enrich_purpose, purpose_search, purpose_judge, digest_validate, concern_judge,
i18n/, migrate/, knowledge/{recall,recall_cli,prune,memory_ingest}, core domain/knowledge.rs,
util/slug.rs); 13 variantes CLI + 13 braços de dispatch removidos; excisão do bloco de recall do
agent_prompt_render; segmento ScanProgress removido do statusline por completo.
Validação: `cargo build/test/clippy --workspace` verdes — 3.699 testes, 0 falhas; grep-gate da
camada removida = zero sobras.

**Prosa/templates:** 3 refs deletados (recall-index, digest-validate, concern-judge); etapa de
vetores removida do /scan (enrich = só Guards); `refs/locating-code.md` reescrito (literal→Grep;
conceito→digest determinístico; camada→Guards); passo do juiz removido de feature/task/bugfix;
seis skills de fluxo (feature, bugfix, task, qa, close, git) com lei de ferro + tabela
anti-racionalização + red flags (Fase 1 concluída). Inventário de comandos/gates conferido
byte a byte: mecânica intocada.

**Dashboard:** src-tauri compila e testa verde contra o core cortado (1 falha pré-existente de
ambiente: `rtk_summary_is_unavailable_on_clean_repo` supõe máquina sem rtk global — dívida de
teste, não regressão). Chamada morta `wikilink-extract` removida do painel (retorna o estado
vazio direto, sem subprocesso fadado a falhar).

**Pendências deixadas de propósito:** comentário morto em feature_outcome_observer.rs:17; teste
rtk env-dependente acima; Fase 4 (convivência superpowers no sialia) não iniciada.
