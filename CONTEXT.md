# CONTEXT.md — Glossário do Projeto

> Glossário canônico do Mustard. Apenas termos de domínio — sem detalhes de implementação.

## Artefato gerenciado (managed artifact)

Arquivo, pasta ou ferramenta de que o Mustard depende e cuja origem é rastreada.
Artefatos vendorados (skill, recipe, ref, command, hook) ficam sob
`apps/cli/templates/` e são copiados para o `.claude/` de um projeto-alvo durante
`init`/`update`. Artefatos `tool` (ex.: o RTK) não são vendorados — são binários
externos instalados na máquina, rastreados apenas por versão.

## Proveniência (provenance)

Registro de onde um artefato gerenciado se originou: o tipo de origem, a
referência upstream, a versão vendorada e o checksum do conteúdo. Materializa-se
no manifest `apps/cli/templates/.artifacts.json`.

## Tipo de origem (source kind)

Classificação da origem de um artefato gerenciado:

- `first-party` — autorado pelo próprio Mustard; sem upstream externo; versiona junto com a CLI.
- `git` — vendorado de um repositório git (repo + subdir + ref).
- `skills-directory` — obtido do registry skills.directory (ex.: `npx skills add nutlope/hallmark`).
- `cargo` — crate publicado no crates.io (ex.: a ferramenta RTK).
- `manual` — vendorado uma vez, sem canal automatizado de atualização.

## artifact-update

Motor maintainer-side (`mustard-rt run artifact-update`) que confere artefatos
gerenciados contra seus upstreams (`--check`) e aplica atualizações dentro de
`templates/` (`--apply`). Nunca toca instalações de usuário.

## Canal de instalação (installation channel)

O `mustard-dashboard` (cf. spec b6): embeda o payload `templates/` no instalador e
roda `init`/`update` nativos. O consumidor recebe os artefatos no estado em que
foram embedados — nunca busca nada na rede.
