# Validação do plugin Mustard num projeto real

Tudo que dá para preparar headless está pronto. Este guia é o passo-a-passo para
você instalar o plugin num Claude Code de verdade e confirmar que ele funciona —
inclusive o único ponto que não é verificável sem uma sessão real (os hooks
disparando via `${CLAUDE_PLUGIN_ROOT}`).

## O que já está pronto (nesta worktree)

- **Binários release compilados e no lugar**: `plugin/bin/mustard-rt.exe` (27,8 MB)
  e `plugin/bin/scan.exe` (16,2 MB). Provados: `run status` e `run feature`
  respondem. (NÃO commitados — são artefato de build, protegidos por `.gitignore`.)
- **Manifesto do marketplace local**: `.claude-plugin/marketplace.json` (nome
  `mustard-local`, aponta `./plugin`).
- **Manifestos do plugin**: `plugin/.claude-plugin/plugin.json`, `hooks/hooks.json`
  (8 eventos → binário embarcado), `.mcp.json` — todos parseiam.
- **Resolução do `.exe` no Windows PROVADA**: a forma shell do `hooks.json`
  (`"${CLAUDE_PLUGIN_ROOT}/bin/mustard-rt" on <Event>`) resolve para o `.exe` via
  PATHEXT do cmd — testado nesta máquina, o binário rodou.

## Smoke test imediato (sem Claude Code, confiança rápida)

Antes de instalar, confirme que o binário do plugin roda no seu terminal:

```
C:\Atiz\mustard\.claude\worktrees\dev_mustard-2-plugado\plugin\bin\mustard-rt.exe run status
```

Deve imprimir o painel de status (Git / Pipelines / Build). Se isso funciona, o
motor está sadio.

## Instalar num projeto real

1. Abra o Claude Code **no seu projeto de teste** (qualquer codebase, não o repo
   do mustard).

2. Registre o marketplace local, apontando para esta worktree (a raiz que contém
   `.claude-plugin/marketplace.json`):
   ```
   /plugin marketplace add C:\Atiz\mustard\.claude\worktrees\dev_mustard-2-plugado
   ```

3. Instale o plugin:
   ```
   /plugin install mustard@mustard-local
   ```

4. Reinicie o Claude Code (ou recarregue) para os hooks do plugin entrarem.

> Nota: para uma instalação de EQUIPE que auto-carrega ao abrir o projeto
> (`enabledPlugins` no `settings.json`), o Claude Code exige que o marketplace
> seja um **repositório git** (não há source de caminho local para
> `extraKnownMarketplaces`). O `mustard init` já escreve esse bloco com um
> placeholder de URL — preencha-o quando publicar o plugin num git privado. Para
> validação local agora, o `/plugin install` acima é o caminho.

## Checklist de validação (numa sessão real)

Confirme, em ordem:

- [ ] **Comandos resolvem** — digite `/mustard:` e veja os 18 comandos
      (`/mustard:scan`, `/mustard:feature`, `/mustard:status`…).
- [ ] **`/mustard:scan`** roda e escreve o `grain.model.json` + os `## Guards` nos
      CLAUDE.md dos subprojetos.
- [ ] **Hooks disparam** — faça uma edição de arquivo; o `post_edit` (auto-format
      + guard) deve agir. Rode um comando destrutivo de teste (ex.: um `git
      reset --hard` num sandbox) e veja o bash-safety negar — prova de que o
      `PreToolUse` está vivo.
- [ ] **Aprovação via plan mode** — inicie uma feature Full; ao sair do plan mode
      (ExitPlanMode), o marcador `.approved-by-user` deve ser cunhado.
- [ ] **SDD (F6)** — o `/mustard:feature` deve gerar ACs em forma **EARS**
      (`when/then/command`), não `build verde`. Um Full não-clarificado deve ser
      **recusado** no approve até rodar a finalização de clarificação.
- [ ] **MCP** — as ferramentas `find_anchors` e `rank_files` aparecem em
      `/mcp` (servidor `mustard-memory`), consultáveis sem subprocesso.
- [ ] **statusline** — a barra de status mostra a linha do `mustard-rt run
      statusline`.

## O que NÃO consigo verificar por você (o limite honesto)

O carregamento do plugin pelo Claude Code e o disparo real dos hooks via
`${CLAUDE_PLUGIN_ROOT}` só se confirmam numa sessão viva — é exatamente o
checklist acima. A estrutura toda foi validada headless (binários rodam,
manifestos parseiam, resolução do `.exe` provada); o que resta é você rodar o
checklist e me dizer o que quebrou, se algo quebrar.

## Depois de validar

- Se tudo passar: o deploy de produção é publicar o `plugin/` num git privado
  (com o release workflow populando `plugin/bin/` por plataforma) e preencher a
  URL do marketplace no `mustard init`. Migração de instalações antigas está em
  `docs/F4-MIGRATION.md`.
- Se algo quebrar: me diga qual item do checklist e o sintoma — conserto o tool
  (não a spec), rebuild, e você revalida.