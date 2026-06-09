# wave-1-impl

## Resumo

Core: seed aspnet no registro + normalizacao de checkbox na materializacao de tasks

## Rede

- Pai: [[menos-ia-mais-mustard-compor]]

## Tarefas

- [ ] - [ ] Em packages/core/src/domain/vocabulary/stacks.toml, adicionar a 4a entry [[stack]] `aspnet` (ecossistema dotnet/nuget): manifest_deps com pacotes NuGet DISTINTIVOS (ex. Microsoft.AspNetCore.OpenApi, Microsoft.EntityFrameworkCore — verifique como o manifesto csproj e parseado em apps/scan/manifests.toml [xml-attr PackageReference Include] para usar a forma que realmente chega nas deps), path_markers (ex. appsettings.json, Program.cs — avalie distintividade; appsettings.json e forte), code_signatures verificadas contra codigo ASP.NET real (ex. `Microsoft.AspNetCore`, `WebApplication.CreateBuilder`, `[ApiController]`). So dado; sem colisao com sinais existentes (dedup first-key-wins).
- [ ] - [ ] Atualizar o teste stacks_registry_parses para >=4 stacks / 4 ecossistemas (php, python, javascript, dotnet).
- [ ] - [ ] Normalizacao de checkbox: localizar onde as tasks de wave sao materializadas com prefixo `- [ ] ` (packages/core/src/domain/spec/contract.rs:117-124 render_checklist_item e/ou o ponto que escreve ## Tarefas no wave spec — siga o fio do wave-scaffold). Fazer strip de prefixo existente `- [ ]`/`- [x]`/`- ` da string de entrada ANTES de prefixar, para plano com tasks ja em formato checkbox nao gerar `- [ ] - [ ]` (defeito medido em 3 specs reais de 2026-06-09).
- [ ] - [ ] Teste `checkbox_normalize_*`: task com prefixo nao duplica; task sem prefixo ganha um; [x] preservado como conteudo-limpo (nao perder o estado? — decisao: input de PLANO e sempre novo, strip de [x] tambem vira [ ]; documente no teste).
- [ ] - [ ] Rodar `cargo test -p mustard-core` completo e reportar numeros (649 hoje).

## Arquivos

- `packages/core/src/domain/vocabulary/stacks.toml`
- `packages/core/src/domain/spec/contract.rs`
