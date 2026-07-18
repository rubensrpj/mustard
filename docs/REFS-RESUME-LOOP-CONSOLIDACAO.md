# Consolidação dos refs de pipeline + conserto do deploy do `pipeline-config.md`

**Data:** 2026-07-01 · **Gatilho:** relatório de campo (`/btw` no projeto sialia, durante a spec `ajustes-grades-contas-pagar-receber`) avaliando a performance do mustard como harness.

## O problema relatado

O agente que *usa* o harness reportou três atritos. Investigados e classificados:

1. **"`pipeline-config.md` é citado como leitura obrigatória mas não existe no repo."** — Factualmente meio-errado sobre a causa. O arquivo **existe e é mantido** na fonte (`apps/cli/templates/pipeline-config.md`). O `init` copia o template inteiro, então projeto recém-criado tem. Mas era um **bug de deploy**: veja abaixo.
2. **Mojibake no Windows** (`jÃ¡`, `necessÃ¡rio`) no `active-specs --format table`. Real. Causa: o binário escreve bytes UTF-8 com `println!`; PowerShell/console do Windows decodifica com code page legado (CP1252/850). **Pendente** (decisão de abordagem — ver Follow-ups).
3. **Cerimônia/indireção pesada** para sair de "aprovar spec" até "agente rodando": `SKILL.md → approve-only-flow.md → resume-flow.md → wave-advance`, com refs que se referenciam, repetem as mesmas "INVIOLABLE RULES" em 3 lugares (às vezes divergindo) e carregam história datada. O procedimento imperativo real é de ~8 linhas, afogado em 227 linhas de prosa que custam token toda vez que um agente carrega o ref.

## Correção 1 — deploy do `pipeline-config.md` (`apps/cli/src/commands/update.rs`)

**Causa-raiz.** O `mustard update` deletava+re-copiava só `commands/mustard`, `skills`, `scripts`, `refs`, `agents`, `settings.json`, e **preservava** (nunca criava nem atualizava) `CLAUDE.md`, `pipeline-config.md`, etc. Consequência dupla:

- **Ausente** em projetos init'd numa versão antiga (antes de o arquivo entrar no template) e só atualizados desde então — o caso do sialia. Todo agente seguindo "leia `pipeline-config.md`" batia num arquivo que nunca foi copiado. Mesmo padrão do bug de `agents/` (memória `project-mustard-update-missing-agents-backfill`).
- **Defasado** nos que tinham: edições no doc de referência do template nunca propagavam (as cópias local×template tinham tamanhos diferentes).

**Conserto.** `pipeline-config.md` é uma referência estática Mustard-owned (sem customização por projeto, ao contrário do `CLAUDE.md` cujos `## Guards` o scan personaliza). Agora o `update` o trata como o `settings.json`: **backfill quando ausente, refresh quando presente**. Novo helper `copy_core_file` (generalização do antigo `copy_settings`, que foi absorvido). Testes: `update_preserves_user_files_and_refreshes_core` (backfill) + `update_refreshes_stale_pipeline_config` (refresh). Verde.

## Correção 3 — consolidação dos refs de fluxo

**Decisão:** colapsar `refs/spec/approve-only-flow.md` (109 linhas) + `refs/spec/resume-flow.md` (118 linhas) *(colapsados em resume-loop.md — layout atual; os dois arquivos não existem mais)* num único **`refs/spec/resume-loop.md`** (~95 linhas), com duas seções marcadas:

- **§A — Approve gate** (`stage=Plan`): render focado, pergunta única de aprovação (com o plano anexado como `preview`), detecção de wave-plan, auditoria de tamanho (advisory), branch de rejeição (contrato `wave-collapse --mode full|light`), `approve-spec`. Se `implementNow=true`, cai direto no §B.
- **§B — O laço** (`stage=Execute`): `wave-advance` como relay (rodada com prompts renderizados + precheck inline), review round, `review-result --subproject`, `wave-done`, `close-pipeline`. Tabela de escalonamento **inline** (6 linhas) — remove a dependência do `pipeline-config.md § Escalation Statuses`.

**Princípio.** O binário já é a fonte da verdade do "o que fazer agora" (`wave-advance`/`resume-bootstrap`/`nextAction`). O ref parou de **re-descrever o procedimento** e virou um **relay fino**: rode o comando, faça o que a saída manda; só o que está marcado `[you]` é decisão do orquestrador. Cortado: história datada ("moved verbatim from…", "as of 2026-05-25…"), regras INVIOLÁVEIS duplicadas do `spec/SKILL.md`/`CLAUDE.md` (mantida só a lista específica do laço).

**Ponteiros atualizados** (template + `.claude/`, mirror): `commands/mustard/spec/SKILL.md`, `commands/mustard/feature/SKILL.md`, `refs/feature/full-plan.md`, `refs/feature/wave-decomposition.md`. Zero órfãos nos docs vivos (verificado por grep; restam só em `.dispatch/`/`spec/*/`/`plans/`, que são histórico congelado).

**Conteúdo antigo** dos dois refs deletados fica preservado no histórico git (não foi copiado verbatim para cá — este ADR é o registro da decisão, o git é o registro do texto).

## Follow-ups

- **Mojibake (Correção 2) — DIAGNOSTICADO E ASSENTADO: não é bug do binário.** Provado por bytes: `active-specs --format table` emite `Estágio` = `45 73 74 C3 A1 …` — `C3 A1` é o UTF-8 **correto** de `á`. Os mesmos bytes decodificados como CP1252 dão `EstÃ¡gio` (o mojibake do relatório). Confirmado ao vivo: consumidor com `[Console]::OutputEncoding` UTF-8 mostra certo; consumidor CP1252 mostra mojibake. **Causa-raiz:** o sistema está em codepage legado (`ACP=1252`, `OEMCP=850`), então todo consumidor que não força UTF-8 (cmd.exe — inclusive o das rodadas de QA —, PowerShell 5.1, capturas por pipe) mangla a saída UTF-8 correta do mustard.
  - **Por que não há conserto no binário:** os bytes já são UTF-8 corretos; `#![forbid(unsafe_code)]` no `apps/rt` proíbe `SetConsoleOutputCP`, que de todo modo não força o consumidor a decodificar UTF-8 (saída em pipe é governada pelo `[Console]::OutputEncoding` do consumidor, não pelo produtor) e teria efeito colateral global (muta o codepage do console compartilhado). Dumbing pra ASCII degradaria a UI PT e quebraria os snapshots byte-estáveis.
  - **Conserto aplicado (parcial):** pin de UTF-8 no perfil do PowerShell 7 do usuário (`Microsoft.PowerShell_profile.ps1`) — cobre sessões PS7 que carregam o perfil.
  - **Conserto definitivo (só o usuário aplica — exige reboot):** habilitar "Beta: Use Unicode UTF-8 for worldwide language support" (Região → Administrativo), que seta `ACP`/`OEMCP=65001` system-wide e mata o mojibake em TODO consumidor (cmd, PS5.1/7, Bash, rtk, captura do Claude Code) de forma permanente.
- **Reforma do contrato (opção C descartada agora)** — fazer `nextAction`/itens carregarem escalonamento/veredito auto-descritos, encolhendo o ref a uma legenda. Toca Rust, precisa de rebuild+QA → spec própria se a dor persistir.
- **Deploy real.** Os consertos vivem na fonte do mustard. Para chegar ao sialia: rebuild+reinstall do CLI, depois `mustard update` no sialia (que agora fará o backfill do `pipeline-config.md`).
