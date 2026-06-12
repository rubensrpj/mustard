# wave-1-backend

## Resumo

Comandos Tauri de git local e overview de projeto no backend do dashboard (camada de dados para a seção Projetos).

## Rede

- Pai: [[redesenho-rota-visao-geral-dashboard]]

## Tarefas

- [ ] Criar git_info.rs: comando que roda git local (remote get-url origin; rev-parse --abbrev-ref HEAD; rev-list --left-right --count para ahead/behind; log -1 para hash/msg/autor/data do último commit) e devolve um struct serde; quando não há repositório ou remote, retornar o struct com campos vazios em vez de erro (card mostra estado vazio).
- [ ] Criar project_overview.rs: ler grain.model.json via mustard_core::read_projects() e projetar { is_monorepo (project_count > 1), project_count, languages, frameworks, detected_stacks }.
- [ ] Editar lib.rs: declarar os módulos git_info e project_overview, definir os comandos dashboard_git_info e dashboard_project_overview e registrá-los no invoke_handler; structs serde com rename camelCase (params repoPath -> repo_path).
- [ ] Seguir o skill dashboard-tauri-pattern (parâmetros camelCase no contrato, snake_case no serde; comando isola git/IO num módulo, lib.rs só registra).

## Arquivos

- `apps/dashboard/src-tauri/src/git_info.rs`
- `apps/dashboard/src-tauri/src/project_overview.rs`
- `apps/dashboard/src-tauri/src/lib.rs`
