---
id: spec.o-provedor-de-git-detectado
---

# o provedor de git é detectado pela URL do remoto em vez de perguntado na instalação

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

o provedor de git é detectado pela URL do remoto em vez de perguntado na instalação.

Hoje o `mustard init` pergunta "Git provider" num menu de três itens e grava a resposta em `mustard.json#git.provider`. A resposta fica lá para sempre.

É a mesma forma do defeito que a unidade anterior corrigiu para as bases: uma resposta congelada no dia da instalação. Só que aqui ela é ainda mais desnecessária, porque está escrita a um comando de distância:

    origin  https://github.com/rubensrpj/mustard.git    -> github
    origin  https://dev.azure.com/suzano/.../_git/x     -> azure
    origin  git@bitbucket.org:time/projeto.git          -> bitbucket

Por que agora: o Mustard passou a ser usado em repositório corporativo hospedado no Azure DevOps, e a lista do menu nem oferece essa opção. Antes de qualquer adaptador, o fato precisa estar certo e vir de onde ele mora.

## Usuários/Stakeholders

Quem instala o Mustard em repositório que não é seu — o provedor deixa de ser mais uma pergunta sobre um projeto que ainda não se conhece. E quem vier depois: nenhum adaptador faz sentido enquanto o valor lido puder estar errado.

## Métrica de sucesso

Instalar em um repositório do Azure DevOps e o Mustard reconhecer `azure` sem pergunta e sem linha de configuração. A contraprova: em instância auto-hospedada, uma sobrescrita explícita continua valendo.

## Não-Objetivos

- **Fazer o Azure DevOps funcionar.** Esta unidade só torna o FATO correto; os três executores continuam chamando `gh` direto.
- **Adivinhar instância auto-hospedada.** O host de um GitHub Enterprise não tem nada de `github`.
- **Remover a chave `git.provider`.** Ela sobrevive como sobrescrita, e é o que salva o caso acima.

## Critérios de Aceitação

- **AC-1** — when o repositório tem um remoto `origin` do Azure DevOps e nenhuma configuração de provedor, then o provedor resolvido é `azure`.
  Command: `cargo test -p mustard-core --lib platform::git_provider::tests::the_provider_comes_from_the_remote_url -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-core --lib platform:: 2>&1 | grep -q "test result: ok"`
- **AC-2** — when `mustard.json#git.provider` traz um valor explícito, then ele vence a detecção — o caso da instância auto-hospedada.
  Command: `cargo test -p mustard-core --lib platform::git_provider::tests::an_explicit_setting_overrides_detection -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-core --lib platform:: 2>&1 | grep -q "test result: ok"`
- **AC-3** — when o `mustard init` roda, then ele não pergunta o provedor e o `mustard.json` gravado não contém a chave `provider`.
  Command: `cargo test -p mustard-cli --lib commands::git_flow::tests::init_does_not_ask_for_the_provider -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
  Control: `cargo test -p mustard-cli --lib commands::git_flow 2>&1 | grep -q "test result: ok"`
- **AC-4** — o build do projeto passa verde
  Command: `cargo build --workspace`

## Checklist

- [ ] T1 — primeira tarefa rastreável.

## Definitions

- **provedor** — quem hospeda os pull requests do repositório — github, gitlab, bitbucket, azure. Hoje é uma string perguntada no init e gravada em mustard.json#git.provider.
- **detecção** — derivar o provedor da URL do remoto `origin`, que o próprio git já guarda, em vez de perguntar.
- **sobrescrita** — um valor explícito em mustard.json#git.provider, que VENCE a detecção. Existe porque instâncias auto-hospedadas (GitHub Enterprise, GitLab CE) não se distinguem pelo nome do host.

## Decisions

- o provedor passa a ser detectado da URL do remoto origin; a chave em mustard.json vira sobrescrita opcional e o init para de perguntá-la
  Reason: é o mesmo defeito que a unidade anterior corrigiu para as bases: uma resposta congelada na instalação, que envelhece e que ninguém revisita. O git já sabe a resposta.
- aqui a CONFIGURAÇÃO vence a detecção, ao contrário do que fizemos com as bases, onde o git vencia
  Reason: são fatos de naturezas diferentes. A lista de bases envelhece porque o repositório muda toda semana; o provedor não muda quase nunca, mas a detecção FALHA de verdade em instância auto-hospedada, onde o host não denuncia o produto. Uma sobrescrita que perde para a detecção seria inútil justamente no caso que a justifica.
- o init para de gravar a chave; um projeto novo nasce sem ela
  Reason: se o init continuasse escrevendo 'github' por padrão, toda instalação nova gravaria uma sobrescrita permanente e a detecção nunca rodaria — o mesmo mecanismo que tornou git.flow uma restrição.
- instância auto-hospedada fica FORA do escopo da detecção e é resolvida pela sobrescrita
  Reason: o nome do host de um GitHub Enterprise não tem nada de github; adivinhar ali seria pior que não responder.

## Evidence

- so existem QUATRO consumidores reais de git.provider fora de teste: dois em git_settle, um em branch_state e um no init do cli
  Evidence: `apps/rt/src/shared/branch_state.rs:580`
- branch_state ja trata provedor nao-github com honestidade: devolve PR_UNSUPPORTED em vez de fingir, e esse e o molde que os outros devem seguir
  Evidence: `apps/rt/src/shared/branch_state.rs:581`
- o init pergunta o provedor num Select de tres itens — uma pergunta cuja resposta esta escrita na URL do remoto
  Evidence: `apps/cli/src/commands/git_flow.rs:131`
- o default do GitConfig grava 'github' explicitamente, entao hoje TODA instalacao nasce com a chave preenchida
  Evidence: `packages/core/src/domain/config.rs:88`
- hipotese refutada: nao ha caminho de PR que escolha ferramenta pelo provider. O gh e chamado direto em tres executores, entao esta unidade NAO faz o Azure funcionar — ela so torna o fato do provedor correto e detectado
  Evidence: `apps/rt/src/commands/review/pr_door.rs:102`
