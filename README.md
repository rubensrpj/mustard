# Mustard

**Português** · [English](README.en.md)

> *Harness* de desenvolvimento de software assistido por IA — impõe um pipeline disciplinado, auditável e econômico em contexto sobre o Claude Code.

O **Mustard** envolve o Claude Code e transforma "peça uma feature para a IA" em um **pipeline orientado a especificação** (Spec-Driven Development / SDD): fases nomeadas, portões bloqueantes e um rastro de eventos auditável. A disciplina não depende da boa vontade do modelo — a **máquina a impõe** via *hooks* e *gates*.

A tese do projeto é **mínimo de IA, máximo de determinismo**: tudo que pode ser resolvido por estatística, grafo ou regra fica num núcleo em Rust; a IA aparece só na orquestração e no raciocínio, nunca embutida no motor.

---

## Princípio central

> **O código-fonte nunca é lido em massa.**

```mermaid
flowchart LR
    repo[("Repositório")] -->|"varredura no porteiro de base (Rust, sem IA)"| model[("grain.model.json")]
    model -->|digest| anchors["~12 anchors<br/>(arquivos-âncora)"]
    anchors -->|"IA lê só estes"| work["pipeline de feature/bugfix"]
```

1. A **varredura** minera o repositório para um modelo durável (`grain.model.json`) — de forma **determinística, sem IA e agnóstica de linguagem/arquitetura**: módulos, declarações, grafo de dependências, *roles*, *slices*, contratos e *touchpoints*. Não é comando: o **porteiro de base** a dispara sozinho quando o censo está velho e a árvore limpa.
2. Os comandos de pipeline consomem esse modelo via **digest** (`mustard-rt run feature`, `scan spec`) e leem apenas as ~12 *anchors* que o digest aponta.
3. Resultado: **economia de contexto** — o digest acha *onde olhar*, não substitui ler.

> O peso real do harness não são os comandos, e sim a **reinjeção da cerimônia no contexto a cada turno**. Por isso o roteamento escolhe sempre o **caminho mais barato que serve** — o pipeline completo é a exceção que precisa se justificar (≥2 camadas/subprojetos **ou** entidade nova), não o default.

---

## Instalação

Pré-requisito único em todos os ambientes: **[Claude Code](https://docs.claude.com/claude-code)** instalado e logado (`claude --version` responde). Você **não** precisa de Rust, Node ou qualquer ferramenta de desenvolvimento — os instaladores trazem tudo pré-compilado.

### Passo 1 — instalador do seu sistema

No Windows e no macOS, baixe **um** arquivo na página de [**Releases**](https://github.com/rubensrpj/mustard/releases) (seção *Assets*); no **Linux**, uma linha de terminal resolve. Cada instalador traz o CLI completo (`mustard`, `mustard-rt`, `mustard-mcp`, `scan`, `rtk`) **e** o **Mustard Dashboard**:

| Sistema | O que baixar | O que fazer |
|---|---|---|
| 🪟 **Windows** 10/11 | `Mustard Dashboard_<versão>_x64-setup.exe` | Duplo-clique. No aviso do SmartScreen (o instalador não é assinado): **"Mais informações" → "Executar assim mesmo"**. Ao final, **abra um terminal novo** — o PATH só vale em terminais abertos depois da instalação. |
| 🍎 **macOS** 11+ (Intel + Apple Silicon) | `Mustard-<versão>-universal.pkg` | O pacote não é assinado: **botão direito → Abrir** (Gatekeeper). Siga o assistente e abra um terminal novo. |
| 🐧 **Linux** (Ubuntu 22.04+) | nenhum — instale numa linha:<br>`curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh \| sh` | O script baixa o `.deb` do último Release e chama o `apt` (que resolve as dependências). Rota manual, para quem quer conferir o `sha256` antes: baixe `mustard_<versão>_amd64.deb` + `install.sh` na mesma pasta e rode `chmod +x install.sh && ./install.sh` — os assets do Release chegam **sem** a permissão de execução, e sem o `chmod` o shell responde `Permission denied`. |

Verifique num terminal novo:

```bash
mustard --version
mustard-rt --version
```

O passo a passo completo de cada sistema (incluindo problemas comuns e desinstalação) está nos *Assets* de cada release: `TUTORIAL-WINDOWS.md`, `TUTORIAL-MACOS.md`, `TUTORIAL-LINUX.md`.

### Passo 2 — plugin no Claude Code

O harness (comandos `/mustard:*`, hooks, gates, agentes e o servidor MCP de memória) é distribuído como **plugin do Claude Code**:

```
/plugin marketplace add rubensrpj/mustard
/plugin install mustard@mustard-local
```

Reinicie (ou recarregue) o Claude Code para os hooks entrarem. O `add` registra o repositório do Mustard como *marketplace* (é ele que traz o `.claude-plugin/marketplace.json`); o `@mustard-local` no `install` é o **nome do marketplace**, não um caminho. O `add` também aceita o caminho de um clone local deste repositório — a raiz que contém `.claude-plugin/marketplace.json` — e a URL completa do repositório (`https://github.com/rubensrpj/mustard.git`), que é a forma a usar quando o atalho `owner/repo` não consegue clonar.

> **Binários automáticos:** o plugin não carrega binários no git. Na **primeira sessão**, o bootstrap (`mustard-boot`) baixa o pacote `mustard-bins-<versão>-<sistema>` dos *Assets* do Release correspondente à versão do plugin e o instala dentro do próprio plugin — silencioso e à prova de falha (sem rede, a sessão segue normal e ele tenta de novo na próxima). Quem instalou pelo Passo 1 já tem o CLI no PATH de qualquer forma; os dois caminhos convivem.

### Passo 3 — preparar um projeto

Na **raiz do repositório git** do seu projeto (o `init` recusa subpastas de um repo — num monorepo, tudo vive na raiz):

```bash
cd /caminho/do/seu/projeto
mustard init
```

Isso cria o `mustard.json` (configuração única) e a pasta `.claude/` (hooks, skills, templates). A partir daí, **abra o Claude Code normalmente dentro do projeto** e **descreva o trabalho em palavras suas** — não há comando para "começar", nem passo de mapeamento para rodar. O roteador é injetado em todo prompt e classifica o pedido sozinho; o porteiro de base minera o repositório no caminho de entrada.

### Para desenvolvedores deste repositório

```powershell
# Compila os binários em release, instala e roda `mustard init` no alvo:
.\install.ps1                  # alvo = diretório atual (com prompt)
.\install.ps1 -Target ..\app   # outro projeto (sem prompt)
```

---

## Pipeline canônico

```mermaid
flowchart LR
    A["ANALYZE"] --> P["PLAN"]
    P -->|/approve| E["EXECUTE"]
    E --> R["REVIEW"]
    R --> Q["QA"]
    Q -->|gate: pass| C["CLOSE"]
```

| Escopo | Detecção | Fluxo |
|---|---|---|
| **Light** | 1-2 camadas, ≤5 arquivos, padrão conhecido | Pula o PLAN: `ANALYZE → EXECUTE → REVIEW → QA → CLOSE` |
| **Full** | 3+ camadas ou entidade nova | Completo, com **aprovação humana** entre PLAN e EXECUTE |

Cada fase emite eventos; os *gates* bloqueiam o avanço. O **close-gate** não deixa fechar sem um `qa.result` com `overall=pass`; editar a spec depois de um QA aprovado marca o pass como *stale* e re-bloqueia até o QA rodar de novo.

---

## Comandos

Instalado como plugin, todo comando vive no namespace `/mustard:`.

### A porta única não é um comando

**Comece descrevendo o trabalho em linguagem natural** — não há comando de entrada. O roteador é injetado em todo prompt: ele classifica o pedido (feature / mudança / correção / investigação + escopo), narra como o leu e despacha o fluxo certo. Só pergunta em ambiguidade genuína.

### As quatro portas

São **quatro**, e só quatro — o que você digita. Todo o resto é fluxo interno que o roteador despacha.

| Comando | Papel |
|---|---|
| `/mustard:spec` | Retoma uma unidade que já tem spec — aprova a planejada, continua a que está em andamento. |
| `/mustard:git` | O trabalho local: sync, commit, push, PR e o ritual de saída. |
| `/mustard:pr` | A porta do pull request: listar, revisar, mergear. |
| `/mustard:upsert` | Instala/atualiza o Mustard no projeto. `--off` / `--on` desligam e religam o harness; `--doctor` diagnostica a instalação. |

#### `/mustard:spec` — a porta da unidade

Uma coisa só: pegar uma unidade que já tem spec e tocar ela adiante. Ele nunca **cria** uma unidade — quem faz isso é o roteador, a partir do seu pedido em linguagem natural.

| Você digita | O que acontece |
|---|---|
| `/mustard:spec` | lista as specs ativas numa tabela e espera a letra |
| `/mustard:spec a` | age na linha `a`: em PLAN aprova, em EXEC continua de onde parou |
| `/mustard:spec ar` | **digitado por inteiro**, aprova *e* implementa no mesmo gesto — sem segunda pergunta |
| `/mustard:spec meu-slug` | vai direto naquela spec, sem tabela |

#### `/mustard:git` — o trabalho local

**Lei de ferro: sobe tudo (`add -A`), nunca um escopo parcial silencioso.** Só operações reversíveis — a única exceção é o `delete`, e é por isso que ele nunca é inferido de uma falha, só digitado.

| Ação | O que faz |
|---|---|
| `sync` | rebase da branch atual na base que o *kind* dela implica; aborta em conflito, jamais força |
| `commit` | cria o commit, sem push |
| `push` | faz `sync`, commita e sobe **apenas a branch atual** |
| `pr [<alvo>]` | abre ou atualiza o PR — idempotente, sempre o mesmo PR. Um por repositório, submódulos antes do pai (enquanto um PR de submódulo estiver aberto, o pai abre como *draft*, e o GitHub recusa mergear *draft*) |
| `pr close` | ritual de saída, rodado da branch de trabalho **depois que o PR mergeou**: volta à base, puxa, remove worktree e apaga a branch local e a remota |
| `delete <branch>` | cancela uma unidade **abandonada**: fecha o PR, remove a worktree, apaga branch local e remota — tudo de uma vez |

A diferença entre os dois últimos é o estado da unidade: `pr close` aposenta uma unidade **entregue**; `delete` cancela uma **abandonada**. PR é o único caminho de integração — uma branch de trabalho nunca chega à base por push direto, e não existe ação `merge` aqui.

#### `/mustard:pr` — a porta do pull request

**Lei de ferro: merge nunca é silencioso.** Mergear uma unidade cuja revisão não voltou `approved` é permitido — quem decide é você, caso a caso — mas é sempre **perguntado** antes, nunca feito calado e nunca recusado de plano.

| Ação | O que faz |
|---|---|
| `list` | os PRs abertos da base onde você está: número, título, se é *draft* e em que branch a unidade vive. Só roda de uma base — "quais PRs estão abertos" é pergunta sobre a base, não sobre uma unidade |
| `review [<pr>]` | revisa **contra a spec da própria unidade** e os moldes daquele subprojeto, e grava o veredito. É esse registro que o merge lê |
| `merge [<pr>] [--confirm]` | cruza o portão de verificação, mergeia e poda: volta à base, puxa, remove worktree e apaga as branches |

O portão que o `merge` cruza, nesta ordem: **build + testes** → **QA** (só um `pass` registrado abre o fechamento) → **review-spans** → **auditoria de docs** → **gates de fechamento**. Passando tudo, a spec é finalizada sozinha — você nunca decide chamar o fechamento à mão.

**Revisão, QA e fechamento não são comandos.** Nenhum deles é o que você saiu para fazer: são o que precisa acontecer no caminho de um merge.

### Fluxos internos (o roteador escolhe)

| Fluxo | Papel |
|---|---|
| varredura | Minera o repositório em `grain.model.json` (determinístico, sem IA) e enriquece os mapas por subprojeto (Guards + moldes de padrão). Disparada pelo porteiro de base. |
| `feature` | Pipeline completo de feature: entende, pesquisa via digest, planeja, implementa. |
| `bugfix` | Diagnóstico + correção autônomos. *Fast path* (1-2 arquivos) ou *full path* (spec enxuta). |
| `tactical-fix` | Cria uma sub-spec ligada a um pai, preservando a pureza do SDD. |
| `task` | Delegação de trabalho sem spec (analyze, audit, refactor, docs…). |

---

## Dashboard

O **Mustard Dashboard** é o aplicativo desktop (Tauri + React) de telemetria do harness: ele lê os eventos NDJSON que os hooks gravam em `.claude/` de cada projeto, **direto do disco e ao vivo** — sem servidor, sem banco de dados, sem depender de sessão aberta.

### Abrir

| Sistema | Como |
|---|---|
| Windows | Menu Iniciar → **"Mustard Dashboard"** |
| macOS | Launchpad / pasta **Aplicativos** → **"Mustard Dashboard"** |
| Linux | Menu de aplicativos → **"Mustard Dashboard"** |

### Primeiro uso

1. Abra **Configurações** (Settings) no menu lateral.
2. Aponte a **pasta-raiz de projetos** — o diretório que contém seus repositórios (ex.: `C:\Atiz` ou `~/code`).
3. O dashboard **descobre sozinho** todo projeto com Mustard iniciado (`mustard.json` + `.claude/`) dentro dela.

### O que cada área mostra

| Área | Conteúdo |
|---|---|
| **Workspace** | Visão geral agregada de todos os projetos descobertos: pipelines ativos, últimos eventos, saúde. |
| **Atividade** | A execução **ao vivo**: pipeline em andamento, ondas, agentes despachados e o trace agrupado por agente/onda. |
| **Specs** | Todas as especificações com o estado do ciclo de vida (ativas, suspeitas, encerradas), critérios de aceitação e ondas. |
| **Economia** | Métricas de tokens: consumo por sessão/spec e a economia obtida (rtk, digest, roteamento). |
| **Conhecimento** | A base de conhecimento do projeto (padrões, convenções, decisões registradas). |
| **Comandos** | Histórico de comandos do pipeline executados. |
| **Sessões** | Histórico de sessões do Claude Code no projeto, com drill-down por sessão. |
| **Detalhe do projeto** | Por projeto: specs, trace de execução e o cartão do pipeline ao vivo. |

> Dica: deixe o dashboard aberto num segundo monitor enquanto o Claude Code trabalha — a aba **Atividade** mostra cada onda e agente em tempo real, e **Specs** reflete os gates (QA aprovado, CLOSE bloqueado etc.) no momento em que acontecem.

---

## Spec-Driven Development

As specs vivem num layout **plano** em `.claude/spec/{name}/`:

- **`spec.md`** — pura narrativa (sem metadata de lifecycle).
- **`meta.json`** — fonte única de verdade do ciclo de vida (`stage` + `outcome` + `flags`). Não há pastas `active/`, `completed/` ou `superseded/`: arquivamento é semântico (um evento `pipeline.status`), não um *move* de filesystem.
- **`wave-plan.md`** + `wave-N-{role}/spec.md` — para o escopo full (uma sub-spec por onda).

Mudanças no meio do caminho são auto-registradas (`change-requests.ndjson` + `change-log.md` legível) — nada se perde, e a narrativa congelada não é tocada.

---

## Arquitetura (monorepo)

| Caminho | Crate/App | Stack | Papel |
|---|---|---|---|
| `apps/rt` | `mustard-rt` | Rust | **Núcleo determinístico** — scan-digest, eventos, gates, hooks, comandos do pipeline. É o motor. |
| `apps/scan` | `scan` | Rust | Minerador do repositório → `grain.model.json`. |
| `apps/cli` | `mustard` | Rust | Instalação e *scaffold* — `init`, gramáticas, git-flow, fontes. |
| `apps/mcp` | `mustard-mcp` | Rust | Servidor MCP (memória/consultas do harness). |
| `packages/core` | `core` | Rust | Tipos e lógica compartilhados (ex.: `ProjectConfig`). |
| `apps/dashboard` | `mustard-dashboard` | Tauri + React | UI de telemetria (specs, runs, trace, métricas). Lê NDJSON; fora do workspace Cargo. |
| `plugin/` | — | — | O plugin do Claude Code: comandos, hooks, agentes, MCP e o bootstrap `mustard-boot` (baixa os binários do Release na primeira sessão). |

O `cargo build --workspace` cobre os crates Rust; o dashboard é construído via `pnpm`.

---

## Build & testes

```bash
# Rust (workspace)
cargo build --workspace            # ou: pnpm build:rust
cargo test  --workspace            # ou: pnpm test:rust
cargo clippy --workspace           # lint

# Dashboard (Tauri + React)
pnpm dashboard:dev                 # dev com HMR
pnpm dashboard:build               # build de produção

# Tudo junto
pnpm build                         # workspace Rust + dashboard
pnpm test                          # idem
```

**Release oficial:** uma tag `vX.Y.Z` dispara o workflow que gera um instalador completo por sistema + os pacotes `mustard-bins-*` (consumidos pelo bootstrap do plugin) e publica tudo num GitHub Release. A versão da tag **deve** bater com `plugin/.claude-plugin/plugin.json` — o workflow recusa tag dessincronizada. O disparo manual (Actions → Release → Run workflow) faz um **ensaio**: builda tudo sem publicar.

---

## Configuração

O `mustard.json` na raiz é a **fonte única** de configuração do projeto:

```jsonc
{
  "git":  { "flow": { "*": "dev", "dev": "main" }, "provider": "github" },
  "buildCommand": "cargo build",
  "testCommand":  "cargo test",
  "lintCommand":  "cargo clippy",
  "typeCheckCommand": "cargo check",
  "specLang": "pt-BR",      // idioma dos artefatos gerados
  "tone":     "didactic"    // tom da prosa gerada
}
```

O Mustard é **agnóstico** de linguagem e de arquitetura: o que é gerado segue `specLang` + `tone`; os comandos de build/test/lint são lidos daqui. Regras de monorepo: todo o estado vive na **raiz** do repositório git; um subprojeto só é um projeto Mustard próprio quando é um repositório git independente (submódulo).

---

## Estrutura do repositório

```
apps/
  rt/         mustard-rt — núcleo determinístico (Rust)
  scan/       minerador do repositório (Rust)
  cli/        mustard — instalador/scaffold (Rust)
  mcp/        servidor MCP (Rust)
  dashboard/  Tauri + React — telemetria
packages/
  core/       tipos/lógica compartilhados (Rust)
plugin/       plugin do Claude Code (comandos, hooks, agentes, bootstrap)
packaging/    instaladores Win/macOS/Linux + tutoriais
docs/         análises e redesenhos arquiteturais
.claude/      config do harness (hooks, skills, refs, specs, grain.model.json)
install.ps1   instalador de desenvolvimento (build + scaffold)
mustard.json  configuração do projeto
```

---

## Documentação

- **[MUSTARD-COMMANDS.md](MUSTARD-COMMANDS.md)** — referência visual de cada comando e seu fluxo (diagramas Mermaid).
- **Tutoriais de instalação** — `packaging/installer/TUTORIAL-{WINDOWS,MACOS,LINUX}.md` (também anexados a cada release).
- **[docs/](docs/)** — redesenhos arquiteturais (índice/digest agnóstico, detecção de stack multissinal, validação do plugin).

---

*Distribuído sob a licença MIT.*
