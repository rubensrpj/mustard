# Enhancement: i18n-foundation-sidebar

### Status: closed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-05-13T00:00:00Z
### Lang: pt

## Contexto

A Sidebar mistura strings PT e EN hardcoded ("Home", "Activity", "Telemetry", "Quality", "Knowledge" em EN; "Workspace", "Tools", "Comandos", "Settings" voltam ao PT) sem nenhum mecanismo de troca de idioma. Como o usuário trabalha em PT mas espera dar acesso a usuários EN no futuro, qualquer mudança nessas strings hoje exige edição direta do componente e força uma escolha permanente. O efeito é que a Sidebar é, na prática, monolíngue acidental — visualmente confusa por ter os dois idiomas misturados e não preparada para a versão EN que precisa existir em paralelo.

## Resumo

Instalar `react-i18next` + `i18next` como foundation de i18n, com strings carregadas inline no `src/i18n.ts` (PT default + EN), persistência do idioma escolhido via Zustand store, Sidebar consumindo via `useTranslation()`, e seletor PT/EN em Settings.

## Limites

- `src/i18n.ts` (novo — init + recursos inline)
- `src/main.tsx` (importar i18n para registrar o provider implícito do react-i18next)
- `src/lib/store.ts` (campo `language: 'pt' | 'en'` + setter)
- `src/components/layout/Sidebar.tsx` (substituir strings literais por `t()`)
- `src/pages/Settings.tsx` (novo Card "Idioma" com seletor PT/EN)
- `package.json` (deps adicionadas: `i18next`, `react-i18next`)
- **Fora do escopo:** Topbar, demais páginas, extração das locales para JSON files — fica para spec futura

## Checklist

### Frontend Agent

- [x] Adicionar deps: `bun add i18next react-i18next` (versões mais recentes compatíveis com React 19 — checar docs antes)
- [x] Criar `src/i18n.ts`: chama `i18n.use(initReactI18next).init({ resources, lng, fallbackLng: 'pt', interpolation: { escapeValue: false } })`. Resources contém `pt.common` e `en.common` inline com chaves: `nav.home`, `nav.activity`, `nav.telemetry`, `nav.quality`, `nav.knowledge`, `nav.commands`, `nav.prd`, `nav.settings`, `group.workspace`, `group.tools`, `tooltip.selectWorkspace`. Exporta `setLanguage(lng)` helper.
- [x] Importar `./i18n` no topo de `src/main.tsx` (antes de qualquer render) para registrar o singleton i18next
- [x] Estender `src/lib/store.ts`: campo `language: 'pt' | 'en'` (default `'pt'`), setter `setLanguage(l)` que chama `i18n.changeLanguage(l)` e persiste — usar useEffect no `main.tsx` ou inicialização lazy para sync inicial após hydrate do persist (sem race)
- [x] Refatorar `src/components/layout/Sidebar.tsx` para usar `const { t } = useTranslation()` e substituir TODAS as strings literais visíveis ("Home", "Activity", "Telemetry", "Quality", "Knowledge", "Workspace", "Tools", "Comandos", "PRD", "Settings", "Selecione um workspace no topo") por `t('nav.*')` ou `t('group.*')` / `t('tooltip.*')` correspondentes
- [x] Adicionar Card "Idioma" em `src/pages/Settings.tsx` (posição: logo após o Card "Diretório de projetos") com 2 botões PT / EN — ativo destacado, click chama `setLanguage()` do store
- [x] `bun run build` e `bun run typecheck` passam; visualmente conferir que trocar idioma atualiza a Sidebar imediatamente (manual)

## Arquivos (~5)

- `src/i18n.ts` (new)
- `src/main.tsx` (modify)
- `src/lib/store.ts` (modify)
- `src/components/layout/Sidebar.tsx` (modify)
- `src/pages/Settings.tsx` (modify)

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Build passa sem erros — Command: `bun run build`
- [x] AC-2: i18n configurado com recursos PT/EN e ambos idiomas têm `nav.home` — Command: `node -e "const fs=require('fs');if(!fs.existsSync('src/i18n.ts'))process.exit(1);const c=fs.readFileSync('src/i18n.ts','utf8');if(!c.includes('initReactI18next'))process.exit(2);if(!(c.includes('pt')&&c.includes('en')))process.exit(3);if(!c.includes('nav.home')&&!(c.match(/home/gi)||[]).length)process.exit(4);console.log('ok')"`
- [x] AC-3: Sidebar consome useTranslation e não tem mais literal "Home" hardcoded — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src/components/layout/Sidebar.tsx','utf8');if(!s.includes('useTranslation'))process.exit(1);if(s.split('\n').some(l=>!l.includes('//')&&/>\s*Home\s*</.test(l)))process.exit(2);console.log('ok')"`
- [x] AC-4: Store expõe campo language e setter — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src/lib/store.ts','utf8');if(!s.includes('language'))process.exit(1);if(!s.includes('setLanguage'))process.exit(2);console.log('ok')"`
- [x] AC-5: Settings inclui Card "Idioma" com setLanguage e mapeamento pt/en — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src/pages/Settings.tsx','utf8');if(!s.includes('setLanguage'))process.exit(1);if(!s.includes('Idioma'))process.exit(2);if(!s.includes(\"'pt'\")||!s.includes(\"'en'\"))process.exit(3);console.log('ok')"`

## Preocupações

- [WARN/layer-gap] `analyze-validation.js` reportou "Spec declares Frontend Agent but Files has no Frontend extensions" — falso positivo: 4 dos 5 arquivos são `.tsx` e um é `.ts`. Não bloqueia.
