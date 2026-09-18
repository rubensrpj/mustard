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
2. Os comandos do fluxo consomem esse modelo via **digest** e leem apenas as ~12 *anchors* que o digest aponta.
3. Resultado: **economia de contexto** — o digest acha *onde olhar*, não substitui ler.

> O peso real do harness não são os comandos, e sim a **reinjeção da cerimônia no contexto a cada turno**. Por isso o roteamento escolhe sempre o **caminho mais barato que serve** — o pipeline completo é a exceção que precisa se justificar (≥2 camadas/subprojetos **ou** entidade nova), não o default.

---

## Instalação

Pré-requisito único em todos os ambientes: **[Claude Code](https://docs.claude.com/claude-code)** instalado e logado (`claude --version` responde). Você **não** precisa de Rust, Node ou qualquer ferramenta de desenvolvimento — os instaladores trazem tudo pré-compilado.

### Passo 1 — instalador do seu sistema

No Windows e no macOS, baixe **um** arquivo na página de [**Releases**](https://github.com/rubensrpj/mustard/releases) (seção *Assets*); no **Linux**, uma linha de terminal resolve. Cada instalador traz o CLI completo (`mustard`, `mustard-rt`, `scan`, `rtk`):

| Sistema | O que baixar | O que fazer |
|---|---|---|
| 🪟 **Windows** 10/11 | `Mustard_<versão>_x64-setup.exe` | Duplo-clique. No aviso do SmartScreen (o instalador não é assinado): **"Mais informações" → "Executar assim mesmo"**. Ao final, **abra um terminal novo** — o PATH só vale em terminais abertos depois da instalação. |
| 🍎 **macOS** 11+ (Intel + Apple Silicon) | `Mustard-<versão>-universal.pkg` | O pacote não é assinado: **botão direito → Abrir** (Gatekeeper). Siga o assistente e abra um terminal novo. |
| 🐧 **Linux** (Ubuntu 22.04+) | nenhum — instale numa linha:<br>`curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh \| sh` | O script baixa o `.deb` do último Release e chama o `apt` (que resolve as dependências). Rota manual, para quem quer conferir o `sha256` antes: baixe `mustard_<versão>_amd64.deb` + `install.sh` na mesma pasta e rode `chmod +x install.sh && ./install.sh` — os assets do Release chegam **sem** a permissão de execução, e sem o `chmod` o shell responde `Permission denied`. |

Verifique num terminal novo:

```bash
mustard --version
mustard-rt --version
```

O passo a passo completo de cada sistema (incluindo problemas comuns e desinstalação) está nos *Assets* de cada release: `TUTORIAL-WINDOWS.md`, `TUTORIAL-MACOS.md`, `TUTORIAL-LINUX.md`.

### Passo 2 — plugin no Claude Code

O harness (comandos `/mustard:*`, hooks, gates e agentes) é distribuído como **plugin do Claude Code**:

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

## O fluxo

```mermaid
flowchart LR
    A["open"] --> G["grill"]
    G --> P["plan"]
    P -->|clique de aprovação| R["round"]
    R --> C["close"]
    C --> PR["pr-open"]
```

Cada passo é uma chamada só, e cada comando termina dizendo qual é o próximo. O `open` abre a spec; o `grill` levanta o que falta, pergunta por pergunta; o `plan` monta as ondas e as põe para aprovação; a aprovação é o clique do usuário, que o gancho da conversa registra; o `round` despacha as ondas que podem sair juntas, cada uma na sua cópia, e grava o que elas entregaram e o veredito de cada revisão; o `close` roda o lint do projeto e cada critério uma vez e, numa spec de duas ondas ou mais, pede a revisão final do conjunto; o `pr-open` abre o pull request. O merge é o único passo que só acontece quando o usuário pede.

O fechamento não fecha enquanto algum critério não tiver a última execução aprovada no `spec.ndjson`, enquanto o lint do projeto falhar, ou enquanto a revisão final do conjunto não tiver sido aprovada.

---

## Comandos

Instalado como plugin, todo comando vive no namespace `/mustard:`. Não há comando de entrada: um pedido que muda arquivo, dito na conversa, abre a spec, e cada passo do fluxo responde qual é o próximo.

| Comando | Papel |
|---|---|
| `/mustard:continue` | Retoma a spec de onde parou. É o botão de reserva: a retomada já acontece no início da sessão. |
| `/mustard:pr` | Abre o pull request, revisa o de um colega ou faz o merge, só a pedido. |
| `/mustard:upsert` | Instala ou atualiza o Mustard no projeto e diagnostica a instalação. Para desligar o Mustard num projeto, ponha `"enabled": false` no `mustard.json`. |

A referência completa — o fluxo, os ganchos e cada comando `mustard-rt run` — está em [`MUSTARD-COMMANDS.md`](MUSTARD-COMMANDS.md).

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
| `packages/core` | `core` | Rust | Tipos e lógica compartilhados (ex.: `ProjectConfig`). |
| `plugin/` | — | — | O plugin do Claude Code: comandos, hooks, agentes e o bootstrap `mustard-boot` (baixa os binários do Release na primeira sessão). |

O `cargo build --workspace` cobre todos os crates Rust.

---

## Build & testes

```bash
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace           # lint
```

**Release oficial:** uma tag `vX.Y.Z` dispara o workflow que gera um instalador completo por sistema + os pacotes `mustard-bins-*` (consumidos pelo bootstrap do plugin) e publica tudo num GitHub Release. A versão da tag **deve** bater com `plugin/.claude-plugin/plugin.json` — o workflow recusa tag dessincronizada. O disparo manual (Actions → Release → Run workflow) faz um **ensaio**: builda tudo sem publicar.

---

## Configuração

O `mustard.json` na raiz é a **fonte única** de configuração do projeto:

```jsonc
{
  // "flow" é OPCIONAL e não restringe nada: ele apenas pré-seleciona a base
  // no seletor. De onde uma unidade pode sair vem do git;
  // onde o commit direto é recusado vem do branch padrão do remoto, mais o que
  // "protected" acrescentar. Uma instalação nova não grava "flow".
  "git":  { "provider": "github" },
  "buildCommand": "cargo build",
  "testCommand":  "cargo test",
  "lintCommand":  "cargo clippy",
  "typeCheckCommand": "cargo check",
  "language": {             // os dois idiomas, cada um na sua chave
    "text": "pt-BR",        // conversa, specs, páginas, comentários e commits
    "code": "en"            // nomes no código: sempre em inglês
  }
}
```

O Mustard é **agnóstico** de linguagem e de arquitetura: o texto gerado segue `language.text`; os nomes no código (variáveis, funções, arquivos, comandos) ficam sempre em inglês, por isso a instalação não pergunta o idioma do código. A instalação pergunta só o idioma do texto e grava só o que você escolher. Os comandos de build/test/lint são lidos daqui. Regras de monorepo: todo o estado vive na **raiz** do repositório git; um subprojeto só é um projeto Mustard próprio quando é um repositório git independente (submódulo).

---

## Estrutura do repositório

```
apps/
  rt/         mustard-rt — núcleo determinístico (Rust)
  scan/       minerador do repositório (Rust)
  cli/        mustard — instalador/scaffold (Rust)
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
