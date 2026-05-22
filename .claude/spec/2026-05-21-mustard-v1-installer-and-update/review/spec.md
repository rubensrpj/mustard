# Review — Mustard v1 installer

### Stage: QaReview
### Outcome: Active
### Flags: 
### Scope: full (wave plan)
### Role: review
### Checkpoint: 2026-05-21T18:00:00Z
### Lang: pt
### Parent: 2026-05-21-mustard-v1-installer-and-update

## Critérios de Aceitação

- [ ] Workspace inteiro compila — Command: `cargo check --workspace`
- [ ] Dashboard tipa e builda — Command: `pnpm --filter mustard-app build`
- [ ] Sem referência stale a `mustard-dashboard` ou `apps/dashboard` em CLAUDE.md/README/scripts — Command: `node -e "const {execSync}=require('child_process');try{const out=execSync('rg --no-heading mustard-dashboard -l',{encoding:'utf8'});if(out.trim()){console.error('stale refs:\\n'+out);process.exit(1)}}catch(e){if(e.status===1)process.exit(0);throw e}"`
- [ ] `.github/workflows/dashboard-release.yml` foi removido — Command: `node -e "if(require('fs').existsSync('.github/workflows/dashboard-release.yml')){process.exit(1)}"`

## Checklist de Review (7 categorias por subproject)

Para cada wave concluída, o review agent (sonnet) deve cobrir:

1. **Correção funcional** — código faz o que a spec diz?
2. **Naming** — convenções consistentes? (snake_case Rust, camelCase TS, kebab-case files)
3. **Erros tratados nos boundaries certos?** — IO, FFI, HTTP retornam Result; lib functions panic apenas em invariantes
4. **Sem comentários "WHAT"** — só comentários WHY (memory: project_code_language_policy)
5. **Cross-platform** — Windows paths, line endings, `process_util::no_window_command` usado em todo Command::new (memory aplicação a esta wave)
6. **Segurança** — não escreve em ~/.claude/settings.json sem MUSTARD_GLOBAL_PERMISSIONS (memory feedback_mustard_install_workflow), não executa shell sem sanitização de path
7. **Sem dependência nova injustificada** — cada dep adicionada tem rationale (em Tarefas ou no PR)
