# Resumo das decisões da sessão — payload, fidelidade, dashboard, porta única

## 0. Contexto
Sessão que começou num `/bugfix` (sialia) e virou uma revisão de fundo: enxugar o payload `.claude/`, ganhar qualidade sem perder nada, e — o fio condutor — garantir que **os `.md` sejam um guia que a IA siga fielmente** (Mustard guia o Claude Code; não pode pular processo), porque **o dashboard depende do fluxo correto** para mostrar o histórico.

## 1. Já feito e COMMITADO — branch `chore/payload-slim` (6 commits, `dev_rubens` intocado)

| Commit | O quê |
|---|---|
| `4827e22d` | **Fase 1 — cortes:** remove skill-creator (~252 KB de Python/HTML/.pyc) dos dois trees, órfão handoff-summary, qa/README; `/skill` re-apontado pra buscar skill-creator sob demanda |
| `d1bf0fd9` | **Fase 2 — CLAUDE.md always-on:** Spec Layout + Context Loading viram ponteiro pro pipeline-config; QA/change-request condensados; Knowledge Capture mantido inline (hook só captura o que a IA emite) |
| `2dfb4a81` | **Triagem de retrieval:** sintoma com âncora literal → grep (pula digest); só-conceito → digest. Lapidação lidera por termo de TRABALHO, não por substantivo de dado. digest-validate não-opcional em digest `strong` com anchors de camada errada |
| `830bf54c` | **Fase 3 — de-dup:** escalation do feature → link pipeline-config (tabela canônica) |
| `fe7d93f1` | **Fase 4 — renames:** `scan-enrich-purpose`→`recall-index`, `glossary-nudge`→`glossary-grill` (todos os inbound atualizados, 0 órfão) |
| `acbd97db` | **Revert (fidelidade):** o de-dup do lexicon-feedback movia uma AÇÃO pra link; restaurado inline em close/task, ref removido |

## 2. Verdades brutais da reauditoria (o que realmente aconteceu)
- **Token de RUNTIME quase não caiu.** O "−258 KB" é **disco/deploy**, não runtime: skill-creator/órfãos **nunca carregam** num pipeline; o de-dup deu **~0** (o mesmo SKILL que roda já puxaria o ref). O custo real de runtime (always-on + SKILLs quentes re-injetados todo turno) mal se moveu.
- **Ganhamos qualidade:** clareza de nomes, deploy limpo (fim do Python no payload), melhor guia de retrieval (triagem/lapidação).
- **Sem perda de conteúdo** — exceto **1 risco aberto:** o re-fetch on-demand do skill-creator **não foi verificado** (pode ter degradado `/skill create|optimize|eval`).
- **De-dup é o lever ERRADO para um guia:** mover prosa-de-ação pra link arrisca a IA pular o passo por ~0 ganho. Por isso o lexicon foi revertido.

## 3. A régua de fidelidade (da doc oficial + reauditoria)
Os `.md` são o **programa** que a IA executa; ela pula prosa sob pressão. Então:
- 🔗 **REFERÊNCIA** (definição, tabela consultada reativamente) → pode virar link **um-nível-de-profundidade**.
- 📌 **AÇÃO / passo crítico** → fica **inline** ("clear steps prevent skipping").
- 🔒 **NÃO-PODE-PULAR** → **mecanismo determinístico**. Mas: **hook que bloqueia ENDURECE/burocratiza** (evitar — só os poucos que já existem: close-gate, qa-gate, scope_guard). O bom "max-Rust" é **composite/observer que EMITE** (não bloqueia; serve dashboard + token + fidelidade de uma vez).

## 4. Dashboard + telemetria
- O dashboard projeta **eventos**; fidelidade ao fluxo = observabilidade. Pular fase/emit → dashboard mente calado.
- **Emissão de evento deve ser side-effect do mecanismo**, não prosa "a IA emite" (senão fura).
- **Separar spec/task/bugfix:** já funciona no nível de **Sessão** (evento `skill.invoked` → `category`, emitido pelo harness). Falta na aba **Specs**: task e bugfix-rápido são spec-less e não têm campo `kind`.
- **Caminhos lean devem emitir telemetria mínima** (acordado): um `pipeline.kind` + o pedido original, como side-effect determinístico.

## 5. Naming / taxonomia
- Os comandos `feature/task/bugfix/tactical-fix` misturam dois eixos (intenção × cerimônia) → confusos. `task` é grab-bag (investigar vs mudar leve); `tactical-fix` é jargão.
- **Decisão:** NÃO renomear os comandos internos (blast-radius alto). O **dashboard fala rótulo humano**, mapeado dos comandos:
  - feature·full → **Nova funcionalidade** · feature·light → **Ajuste/melhoria** · bugfix → **Correção** · tactical-fix → **Follow-up pontual** · task(analyze/audit/review/docs) → **Investigação** · task(implement/refactor) → **Mudança rápida**.

## 6. DECISÃO PRINCIPAL — porta única que roteia + confirma; comandos ocultos
- **Uma porta de entrada em linguagem natural:** o usuário descreve o que quer; o mustard **classifica** (route + scope, com a inteligência que já tem) e **confirma na dúvida** ("entendi como ajuste pequeno [leve], confirma?" = o "nunca fazer sem questionar").
- **Comandos `/mustard:feature` etc. ficam OCULTOS** — viram máquina interna, não a escolha do usuário. (`tactical-fix` já é interno: nasce no review/QA.)
- **Princípio de coerência (a preocupação do usuário):** separar duas audiências na doc para a IA NÃO se confundir:
  - **Superfície do usuário** = UMA porta (descreva o que quer).
  - **Camada interna** = a IA continua rodando os MESMOS fluxos nomeados (feature/bugfix/task); a **Intent Routing do `CLAUDE.md` é a fonte única** de "intenção do usuário → fluxo interno". Os SKILLs seguem documentando os fluxos (interno), só deixam de ser anunciados como escolha.
  - Regra: doc-de-usuário fala da porta única; doc-interna (SKILLs/CLAUDE.md) fala dos fluxos + do roteador. Não misturar — é o que evita a confusão.

## 7. Trabalho em aberto (priorizado)
1. **Fechar o loss-risk do skill-creator** — verificar `/skill create` ou re-bundlar. (segurança)
2. **Porta única** — implementar route+confirm, ocultar comandos, manter a Intent Routing como fonte única + doc em duas camadas.
3. **Telemetria lean** — `pipeline.kind` + pedido original, side-effect determinístico (task + bugfix-fast).
4. **Dashboard "Atividade"** — substitui aba Specs; agrupa por rótulo humano; cada item mostra o **pedido original + narrativa/histórico** (o que resolve o "fico às cegas").
5. **Token de verdade, sem skip-risk** — apertar a prosa dos SKILLs quentes (qualidade) + mover EMISSÃO de evento pro mecanismo (composite/observer).

## 8. Estado do repositório
- Branch `chore/payload-slim`, 6 commits acima; `dev_rubens` intocado; trabalho Rust/scan/Cargo pré-existente **não-staged** (não é meu).
- Docs de planejamento: `ENXUGAR-PAYLOAD-CLAUDE.md`, `PLANO-TOKEN-FIDELIDADE.md`, este resumo.
- Nada instalado no sialia ainda (deploy futuro via `install.ps1` → `update --force` propaga SKILLs/refs).
