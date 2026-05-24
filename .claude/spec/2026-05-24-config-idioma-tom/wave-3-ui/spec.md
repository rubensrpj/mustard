# Onda 3 — Dashboard e descrições de skill

## Resumo

Esta onda fecha o ciclo no que o usuário vê na tela. O dashboard ganha controles para editar `lang`+`tone` por projeto. Os componentes que mostram dados do projeto consomem essa configuração. As descrições das skills (que aparecem em `/help`) são geradas no idioma escolhido durante `mustard init`/`mustard update`.

## O que muda neste passo

1. **Página Settings ganha dois seletores.** Em `apps/dashboard/src/pages/Settings.tsx`, dois `<select>` dentro da seção do projeto ativo: "Idioma do projeto" (pt-BR | en-US) e "Tom do projeto" (didático | técnico | caveman). Cada mudança grava no `.claude/mustard.json` via comando Tauri novo (`set_project_config`).

2. **Página Preferences fica intocada por dentro, mas ganha uma nota.** Em `apps/dashboard/src/pages/Preferences.tsx`, adicionamos um bloco de texto visível: "Esta página controla apenas o idioma visual do dashboard (sidebar, menus, botões). Para mudar o idioma das specs e dos avisos do Mustard, vá em Settings com um projeto selecionado." Sem alterar a lógica do zustand.

3. **Comando Tauri novo: `set_project_config`.** Em `apps/dashboard/src-tauri/src/commands/config.rs`, recebe `path` + chave + valor. Lê o `mustard.json` do projeto, escreve preservando todas as outras chaves. Atômico (escreve em `.tmp` e renomeia).

4. **Hook React `useProjectLang(path)`.** Em `apps/dashboard/src/hooks/useProjectLang.ts`, devolve o `lang` do projeto via TanStack Query, com key por `path`.

5. **Componentes consomem `lang` do projeto ativo.** `LivePipelineCard`, `AggregateOverview`, `SpecTrackRow` chamam `useProjectLang`. Labels deixam de ser hardcoded em inglês. "Wave 3" vira "Onda 3" quando lang=pt-BR; "RTK saved" vira "Tokens economizados".

6. **Padronização `W3` vs `onda 3`.** Função `formatWave(n, total, lang)` em `apps/dashboard/src/features/workspace/_shared/formatWave.ts`. Os componentes irmãos passam a usá-la.

7. **Descrições de skill bilíngues.** Cada `SKILL.md` em `apps/cli/templates/commands/mustard/**/` ganha duas versões do campo `description:` no frontmatter (`description.pt-BR` e `description.en-US`). Na hora de `mustard init`/`update`, o gerador escolhe a versão certa baseado em `lang` do projeto.

## Arquivos

- `apps/dashboard/src/pages/Settings.tsx` — dois seletores.
- `apps/dashboard/src/pages/Preferences.tsx` — nota informativa.
- `apps/dashboard/src-tauri/src/commands/config.rs` (novo) — `set_project_config`.
- `apps/dashboard/src-tauri/src/lib.rs` — registra o handler.
- `apps/dashboard/src/hooks/useProjectLang.ts` (novo).
- `apps/dashboard/src/features/workspace/_shared/formatWave.ts` (novo).
- `apps/dashboard/src/features/workspace/LivePipelineCard/index.tsx` — usa `useProjectLang` + `formatWave`.
- `apps/dashboard/src/features/workspace/AggregateOverview/index.tsx` — labels traduzidas.
- `apps/dashboard/src/features/workspace/SpecTrackRow/index.tsx` — usa `formatWave`.
- `apps/cli/templates/commands/mustard/**/SKILL.md` — frontmatter bilíngue.
- `apps/cli/src/commands/init.rs`, `update.rs` — gerador escolhe `description` por `lang`.

## Component Contract

Página Settings, dois seletores:

- **Props:** `projectPath: string`.
- **Estados:** `loading`, `error`, `ready`, `saving`.
- **Variantes:** tamanho padrão (sem variantes de densidade).
- **Breakpoints:** stack vertical em mobile/sm, grid 2-colunas em md+.
- **A11y:** `<label htmlFor>`, `aria-busy` durante saving, foco visível.
- **DS tokens:** `color.fg.default`, `color.bg.surface`, `spacing.4`, `typography.body.sm`.
- **Microinterações:** spinner inline durante saving; toast verde ao salvar; toast vermelho ao falhar (mensagem amigável da camada `tr!` da Onda 2).

## Tarefas

### UI Agent (Wave 3)

- [ ] Criar `set_project_config(path, key, value)` em `apps/dashboard/src-tauri/src/commands/config.rs`. Atômico.
- [ ] Registrar no `lib.rs` do `src-tauri` (lista `tauri::generate_handler!`).
- [ ] Criar hook `useProjectLang(projectPath)` em `apps/dashboard/src/hooks/useProjectLang.ts`. Devolve `'pt-BR' | 'en-US'`.
- [ ] Criar `apps/dashboard/src/features/workspace/_shared/formatWave.ts` exportando `formatWave(n, total, lang) -> string`.
- [ ] Em `Settings.tsx`, adicionar dois `<select>` (idioma + tom). Cada mudança chama `invoke('set_project_config', ...)`.
- [ ] Em `Preferences.tsx`, adicionar bloco de nota explicando o escopo. **NÃO** chamar `set_project_config`. **NÃO** importar nada relacionado a `mustard.json`.
- [ ] Refatorar `LivePipelineCard/index.tsx` para usar `useProjectLang` + `formatWave`. Remover string literal `W{n}`.
- [ ] Refatorar `AggregateOverview/index.tsx`: trocar labels hardcoded por chaves do i18next.
- [ ] Refatorar `SpecTrackRow/index.tsx` para usar `formatWave`.
- [ ] Reescrever frontmatter dos `SKILL.md` em `apps/cli/templates/commands/mustard/**/SKILL.md` com `description.pt-BR` e `description.en-US`.
- [ ] No `init.rs`/`update.rs`, gerador lê `lang` e escolhe a versão do `description`.
- [ ] `pnpm --filter mustard-dashboard build` e lint passam.
- [ ] AC-5, AC-6, AC-8 do wave-plan passam.

## Dependências

Depende da Onda 1 (precisa de `lang`+`tone` existirem no `mustard.json` e do hook `useProjectLang` consumir esse arquivo). Não depende da Onda 2 — pode rodar em paralelo.

Também depende da spec B (`2026-05-24-meta-sidecar`) — herda o terreno simplificado.

## Limites

Esta onda **só** mexe nos arquivos do dashboard e nos `SKILL.md` dos comandos. Não toca nos hooks/banners do `mustard-rt` nem nos outputs do `mustard-cli` (esses são da Onda 2).

## Preocupações

- **Layout em PT.** Algumas labels podem ficar maiores em pt-BR ("Tokens economizados" vs "RTK saved"). Conferir responsividade dos cards.
- **Cache stale.** `useProjectLang` precisa invalidar a query quando o usuário trocar idioma via Settings — `queryClient.invalidateQueries(['project-lang', path])` no callback de sucesso.
