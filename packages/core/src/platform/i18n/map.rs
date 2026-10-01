//! O mapa do projeto: as recusas e os motivos do comando `map` e o
//! `scan-map`.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["map", "scan"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // O `.claude/scan-map.md`, o arquivo que a máquina escreve
        // (`commands/scan_claude.rs::render_map`). Ele é MOSTRADO ao
        // desenvolvedor e lido pelo modelo, então segue a língua do texto do
        // projeto (`language.text` no `mustard.json`) — ao contrário do
        // censo, do índice e da busca internos, que ficam em inglês por
        // política. As vagas `{kind}` e `{count}` são preenchidas por quem
        // chama.
        // A dica do mapa diz que o `Grep`, o `grep` e o `rg` passam pelo
        // Mustard, que responde no lugar da busca, e o que cada marca da
        // resposta quer dizer. Não pede palavra nem frase à parte: o texto é o
        // da busca de sempre.
        ("scan.map.type_line", Locale::PtBr) => "Tipo: {kind} · {count} arquivos",
        ("scan.map.type_line", Locale::EnUs) => "Type: {kind} · {count} files",
        ("scan.map.pointer", Locale::PtBr) => {
            "Procure código como sempre, com o mesmo texto: `Grep`, `grep` e `rg` passam pelo Mustard, que responde no lugar da busca. A resposta vem agrupada por função, com o arquivo, as linhas de começo e fim e o código. Cravado: o mapa achou pelo nome, e a resposta já traz o trecho. Parcial: o mapa achou parte, a busca roda, e a nota diz o que falta. Não achei: a busca comum roda, com uma linha do que o mapa não achou. A busca que só lista nomes de arquivo ou conta roda como veio, com uma linha da marca. A mesma busca, repetida, passa. Para pedir a resposta sem buscar, rode `mustard-rt run map search \"<padrão>\"`. Depois leia os arquivos apontados: o mapa acha onde olhar, não substitui ler."
        }
        ("scan.map.pointer", Locale::EnUs) => {
            "Search for code as always, with the same text: `Grep`, `grep` and `rg` go through Mustard, which answers in place of the search. The answer is grouped by function, with the file, the first and last lines and the code. Pinned: the map found it by name, and the answer already carries the excerpt. Partial: the map found part, the search runs, and the note says what is missing. Found nothing: the plain search runs, with one line of what the map did not find. A search that only lists file names or counts runs as it came, with one line of the mark. The same search, repeated, passes. To get the answer without searching, run `mustard-rt run map search \"<pattern>\"`. Then read the files it points to: the map finds where to look, it does not replace reading."
        }
        // O teto do nome comum escrito errado no `mustard.json`: sai na
        // resposta do scan e na de quem usa, uma vez por sessão.
        ("scan.bad_max_same_name", Locale::PtBr) => {
            "O valor {value} de scan.max_same_name no mustard.json não vale, porque o número tem que ser inteiro e \
             maior que zero. O scan usa o padrão, {default}."
        }
        ("scan.bad_max_same_name", Locale::EnUs) => {
            "The value {value} of scan.max_same_name in mustard.json does not count, because the number must be a \
             whole number above zero. The scan uses the default, {default}."
        }
        // The project map (`run map`): refusals, reasons of the examples and
        // the summary of the map.
        ("map.missing", Locale::PtBr) => "O mapa do projeto ainda não existe. Rode `mustard-rt run scan`.",
        ("map.missing", Locale::EnUs) => "The project map does not exist yet. Run `mustard-rt run scan`.",
        ("map.unreadable", Locale::PtBr) => {
            "O mapa do projeto não pôde ser lido ({detail}). Rode `mustard-rt run scan` de novo."
        }
        ("map.unreadable", Locale::EnUs) => {
            "The project map could not be read ({detail}). Run `mustard-rt run scan` again."
        }
        ("map.unfilled", Locale::PtBr) => {
            "Os blocos {blocks} do mapa voltaram vazios numa troca de formato, e o scan ainda não os encheu de novo. \
             Rode `mustard-rt run scan`. Se a recusa voltar, o scan ao lado do mustard-rt é de outra compilação: \
             compile ou instale os dois juntos."
        }
        ("map.unfilled", Locale::EnUs) => {
            "The map blocks {blocks} came back empty in a format change, and the scan has not filled them again. \
             Run `mustard-rt run scan`. If this refusal comes back, the scan beside mustard-rt is from another build: \
             build or install both together."
        }
        ("map.unknown_file", Locale::PtBr) => {
            "O arquivo `{file}` não está no mapa. Confira o caminho a partir da raiz do projeto, ou \
             rode `mustard-rt run scan` se ele é novo."
        }
        ("map.unknown_file", Locale::EnUs) => {
            "The file `{file}` is not in the map. Check the path from the project root, or run \
             `mustard-rt run scan` if it is new."
        }
        ("map.unknown_declaration", Locale::PtBr) => {
            "O arquivo `{file}` não declara `{name}`. Confira o nome, ou rode `mustard-rt run scan` \
             se ele é novo."
        }
        ("map.unknown_declaration", Locale::EnUs) => {
            "The file `{file}` declares no `{name}`. Check the name, or run `mustard-rt run scan` \
             if it is new."
        }
        ("map.unknown_name", Locale::PtBr) => {
            "O mapa não tem declaração chamada `{name}`. Confira o nome, ou rode `mustard-rt run scan` \
             se ela é nova."
        }
        ("map.unknown_name", Locale::EnUs) => {
            "The map has no declaration named `{name}`. Check the name, or run `mustard-rt run scan` \
             if it is new."
        }
        ("map.file_unreadable", Locale::PtBr) => {
            "O arquivo `{file}` está no mapa e não pôde ser lido ({detail}). Confira se ele ainda \
             está no lugar."
        }
        ("map.file_unreadable", Locale::EnUs) => {
            "The file `{file}` is in the map and could not be read ({detail}). Check whether it is \
             still there."
        }
        ("map.changed_in_copy", Locale::PtBr) => {
            "A declaração `{name}` de `{file}` mudou nesta cópia depois do mapa, que tem a do projeto a partir da \
             linha {line}. Leia o arquivo desta cópia por faixa de linhas."
        }
        ("map.changed_in_copy", Locale::EnUs) => {
            "The declaration `{name}` in `{file}` changed in this copy after the map, which has the project's one \
             from line {line}. Read the file of this copy by a line range."
        }
        ("map.changed_in_copy_range", Locale::PtBr) => {
            "A declaração `{name}` de `{file}` mudou nesta cópia depois do mapa. Nesta cópia ela está entre as \
             linhas {first} e {last}. Leia essa faixa do arquivo."
        }
        ("map.changed_in_copy_range", Locale::EnUs) => {
            "The declaration `{name}` in `{file}` changed in this copy after the map. In this copy it sits between \
             lines {first} and {last}. Read that range of the file."
        }
        ("map.missing_argument", Locale::PtBr) => "A pergunta `{question}` precisa de `{flag}`.",
        ("map.missing_argument", Locale::EnUs) => "The `{question}` question needs `{flag}`.",
        ("map.skill_unreadable", Locale::PtBr) => "A skill `{path}` não pôde ser lida ({detail}).",
        ("map.skill_unreadable", Locale::EnUs) => "The skill `{path}` could not be read ({detail}).",
        ("map.skill_missing_path", Locale::PtBr) => {
            "A skill cita caminhos que não existem: {paths}. Corrija o caminho ou tire a citação."
        }
        ("map.skill_missing_path", Locale::EnUs) => {
            "The skill cites paths that do not exist: {paths}. Fix the path or drop the citation."
        }
        ("map.skill_too_long", Locale::PtBr) => {
            "A skill tem {lines} linhas, e o limite é {max}. Corte o que não ajuda a tarefa."
        }
        ("map.skill_too_long", Locale::EnUs) => {
            "The skill has {lines} lines, and the limit is {max}. Cut what does not help the task."
        }
        ("map.no_target", Locale::PtBr) => {
            "Nenhum arquivo do mapa casa com a tarefa. Diga o arquivo que ela cria ou muda com `--file`."
        }
        ("map.no_target", Locale::EnUs) => {
            "No file in the map matches the task. Name the file it creates or changes with `--file`."
        }
        ("map.no_examples", Locale::PtBr) => "Nenhum arquivo da pasta `{folder}` serve de exemplo.",
        ("map.no_examples", Locale::EnUs) => "No file in the folder `{folder}` serves as an example.",
        ("map.why.same_folder", Locale::PtBr) => "na mesma pasta",
        ("map.why.same_folder", Locale::EnUs) => "in the same folder",
        ("map.why.near_folder", Locale::PtBr) => "numa pasta vizinha (a pasta tem menos de 2 exemplos)",
        ("map.why.near_folder", Locale::EnUs) => "in a neighbouring folder (the folder has fewer than 2 examples)",
        ("map.why.imports", Locale::PtBr) => "{shared} de {of} importações principais em comum",
        ("map.why.imports", Locale::EnUs) => "{shared} of {of} main imports in common",
        ("map.why.tested", Locale::PtBr) => "coberto por {tests}",
        ("map.why.tested", Locale::EnUs) => "covered by {tests}",
        ("map.why.inline_tests", Locale::PtBr) => "tem testes no próprio arquivo",
        ("map.why.inline_tests", Locale::EnUs) => "has tests in the file itself",
        ("map.why.recent", Locale::PtBr) => "mudado em {date}",
        ("map.why.recent", Locale::EnUs) => "changed on {date}",
        ("map.why.size", Locale::PtBr) => "tamanho típico da pasta ({loc} linhas)",
        ("map.why.size", Locale::EnUs) => "typical size for the folder ({loc} lines)",
        ("map.summary.head", Locale::PtBr) => "Mapa do projeto: {files} arquivos de código ({languages}).",
        ("map.summary.head", Locale::EnUs) => "Project map: {files} code files ({languages}).",
        ("map.summary.projects", Locale::PtBr) => "Subprojetos:",
        ("map.summary.projects", Locale::EnUs) => "Subprojects:",
        ("map.summary.project_line", Locale::PtBr) => "- {name} (`{dir}`, {kind}, {files} arquivos)",
        ("map.summary.project_line", Locale::EnUs) => "- {name} (`{dir}`, {kind}, {files} files)",
        ("map.summary.hubs", Locale::PtBr) => "Mais importados: {files}.",
        ("map.summary.hubs", Locale::EnUs) => "Most imported: {files}.",
        ("map.summary.recent", Locale::PtBr) => "Mudados há pouco: {files}.",
        ("map.summary.recent", Locale::EnUs) => "Recently changed: {files}.",
        ("map.summary.ask", Locale::PtBr) => {
            "Pergunte ao mapa: `mustard-rt run map examples --file <caminho>`, `importers`, `tests`, \
             `summary --file <caminho>`, `slice --file <caminho> --name <declaração>`, `users --name <declaração>`, \
             `history --name <declaração>` ou `search \"<padrão>\"`."
        }
        ("map.summary.ask", Locale::EnUs) => {
            "Ask the map: `mustard-rt run map examples --file <path>`, `importers`, `tests`, \
             `summary --file <path>`, `slice --file <path> --name <declaration>`, `users --name <declaration>`, \
             `history --name <declaration>` or `search \"<pattern>\"`."
        }
        ("map.users.head", Locale::PtBr) => {
            "Quem usa `{name}`, como arquivo:linha:quem chama. As ligações provadas vêm primeiro. \
             As suspeitas vêm depois, agrupadas pelas declarações que a chamada pode alcançar."
        }
        ("map.users.head", Locale::EnUs) => {
            "Who uses `{name}`, as file:line:caller. Proven links come first. \
             Suspect ones follow, grouped by the declarations the call may reach."
        }
        ("map.users.none", Locale::PtBr) => "Ninguém usa `{name}` de `{file}`.",
        ("map.users.none", Locale::EnUs) => "Nothing uses `{name}` from `{file}`.",
        ("map.users.suspect", Locale::PtBr) => {
            "Uma ligação suspeita tem mais de um alvo possível, ou chega por um valor de tipo que o mapa não conhece. \
             Para decidir, peça goToDefinition na linha de cada chamada à ferramenta `LSP` (servidor de linguagem) \
             do Claude Code."
        }
        ("map.users.suspect", Locale::EnUs) => {
            "A suspect link has more than one possible target, or comes through a value whose type the map does not know. \
             To decide, ask the `LSP` (language server) tool of Claude Code for goToDefinition on the line of each call."
        }
        ("map.users.routes", Locale::PtBr) => {
            "As rotas do servidor que `{name}` atende vêm em routes. Cada uma traz as chamadas da tela que a \
             alcançam, como arquivo:linha:quem chama."
        }
        ("map.users.routes", Locale::EnUs) => {
            "The server routes that `{name}` serves come in routes. Each one lists the screen calls that reach it, \
             as file:line:caller."
        }
        ("map.users.route_suspect", Locale::PtBr) => {
            "Uma chamada suspeita da tela casa com mais de uma rota. Ou só casa sem a versão do endereço, como v1, \
             ou sem a base do cliente. Para decidir, leia o endereço na linha da chamada."
        }
        ("map.users.route_suspect", Locale::EnUs) => {
            "A suspect screen call matches more than one route. Or it only matches without the address version, \
             such as v1, or without the client base. To decide, read the address on the line of the call."
        }
        ("map.users.common.one", Locale::PtBr) => {
            "Uma chamada de `{name}` ficou sem ligação, porque o nome é comum demais para o mapa decidir. \
             Para achá-la, peça findReferences nesta declaração à ferramenta `LSP` (servidor de linguagem) \
             do Claude Code."
        }
        ("map.users.common.one", Locale::EnUs) => {
            "One call of `{name}` was left unlinked, because the name is too common for the map to decide. \
             To find it, ask the `LSP` (language server) tool of Claude Code for findReferences on this declaration."
        }
        ("map.users.common.many", Locale::PtBr) => {
            "{count} chamadas de `{name}` ficaram sem ligação, porque o nome é comum demais para o mapa decidir. \
             Para achá-las, peça findReferences nesta declaração à ferramenta `LSP` (servidor de linguagem) \
             do Claude Code."
        }
        ("map.users.common.many", Locale::EnUs) => {
            "{count} calls of `{name}` were left unlinked, because the name is too common for the map to decide. \
             To find them, ask the `LSP` (language server) tool of Claude Code for findReferences on this declaration."
        }
        ("map.history.no_base", Locale::PtBr) => {
            "O mapa está sem a história do git, porque o projeto não diz qual é a branch de partida. \
             É a branch que recebe os pull requests. Para declarar, ponha o nome dela em mustard.json, \
             em git.flow, na chave \"*\"."
        }
        ("map.history.no_base", Locale::EnUs) => {
            "The map has no git history, because the project does not name its base branch. \
             It is the branch that takes the pull requests. To name it, put its name in mustard.json, \
             in git.flow, under the key \"*\"."
        }
        ("map.history.base_not_found", Locale::PtBr) => {
            "O mapa está sem a história do git, porque a branch de partida {base} não existe neste clone. \
             Ela falta aqui e em origin. Para trazer, rode git fetch origin {base}."
        }
        ("map.history.base_not_found", Locale::EnUs) => {
            "The map has no git history, because the base branch {base} is not in this clone. \
             It is missing here and in origin. To get it, run git fetch origin {base}."
        }
        // A história de uma declaração (`run map history`).
        ("map.history.head", Locale::PtBr) => {
            "Mudanças de `{name}` ({file}:{line}) na base {base}, fora as só de forma: {count}. \
             Os commits vêm do mais novo ao mais velho."
        }
        ("map.history.head", Locale::EnUs) => {
            "Changes to `{name}` ({file}:{line}) on the base {base}, not counting format-only ones: {count}. \
             The commits run from newest to oldest."
        }
        ("map.history.form", Locale::PtBr) => "(só forma)",
        ("map.history.form", Locale::EnUs) => "(format only)",
        ("map.history.next", Locale::PtBr) => "Para ver a mudança mais nova, rode git show {commit}.",
        ("map.history.next", Locale::EnUs) => "To see the newest change, run git show {commit}.",
        ("map.history.not_in_base", Locale::PtBr) => {
            "A base {base} ainda não tem commit de `{name}` em {file}. Ela só existe na branch de trabalho."
        }
        ("map.history.not_in_base", Locale::EnUs) => {
            "The base {base} has no commit of `{name}` in {file} yet. It only exists on the work branch."
        }
        ("map.history.pick_file", Locale::PtBr) => {
            "`{name}` existe em mais de um arquivo. Diga qual com --file."
        }
        ("map.history.pick_file", Locale::EnUs) => "`{name}` exists in more than one file. Say which one with --file.",
        ("map.history.bad_setting", Locale::PtBr) => {
            "O valor de map.{key} no mustard.json não vale, porque o número tem que ser inteiro e maior que zero. \
             A história usa o padrão, {default}."
        }
        ("map.history.bad_setting", Locale::EnUs) => {
            "The value of map.{key} in mustard.json does not count, because the number must be a whole number \
             above zero. The history uses the default, {default}."
        }
        ("map.history.pull_missing", Locale::PtBr) => {
            "O mapa ainda não tem o texto do pull request #{number}. Ele vem do provedor quando o mapa se \
             atualiza, depois que o commit entra na base."
        }
        ("map.history.pull_missing", Locale::EnUs) => {
            "The map does not have the text of pull request #{number} yet. It comes from the provider when \
             the map updates, after the commit reaches the base."
        }
        ("map.history.spec", Locale::PtBr) => "combinado na spec {spec}, {code}: {sentence}",
        ("map.history.spec", Locale::EnUs) => "agreed in spec {spec}, {code}: {sentence}",
        // A busca com filtro: a frase da busca de uma palavra só, os avisos
        // do filtro e dos números da seção `search`, e o motivo de cada falha
        // do filtro.
        // A busca sem nenhum achado: a linha que diz que não achou, manda
        // seguir com as ferramentas padrões e dá, para começar, a busca
        // exata, com as palavras já quebradas.
        ("map.search.not_found", Locale::PtBr) => {
            "Não achei {words} no mapa. Siga com suas ferramentas: `Grep`, `Glob` e `Read`. Para começar, busque o texto exato: {next}"
        }
        ("map.search.not_found", Locale::EnUs) => {
            "Found nothing for {words} in the map. Go on with your tools: `Grep`, `Glob` and `Read`. To start, search the exact text: {next}"
        }
        // A busca que o mapa não responde, perguntada pelo comando: a pasta
        // fora do código do mapa, o padrão que a leitura não entende e o tipo
        // que ela não conhece.
        ("map.search.pass", Locale::PtBr) => {
            "O mapa não tem resposta para esta busca. Ela fica fora das pastas de código do mapa, ou o mapa não lê este padrão. Siga com a busca comum."
        }
        ("map.search.pass", Locale::EnUs) => {
            "The map has no answer for this search. It falls outside the code folders of the map, or the map cannot read this pattern. Go on with the plain search."
        }
        // A resposta no lugar da busca por palavra: a marca com o que o mapa
        // achou, o que faltou, o aviso do arquivo mudado e a contagem do que
        // o corte deixou de fora.
        ("map.answer.pinned", Locale::PtBr) => {
            "Cravado. O mapa achou {words} pelo nome."
        }
        ("map.answer.pinned", Locale::EnUs) => {
            "Pinned. The map found {words} by name."
        }
        ("map.answer.partial", Locale::PtBr) => {
            "Parcial. O mapa achou parte. Falta {missing}. É o que o mapa achou, ao lado do resultado da busca."
        }
        ("map.answer.partial", Locale::EnUs) => {
            "Partial. The map found part. It lacks {missing}. This is what the map found, next to the search result."
        }
        ("map.answer.partial_unsure", Locale::PtBr) => {
            "Parcial. O mapa achou, mas não tem certeza de que este é o lugar. É o que o mapa achou, ao lado do resultado da busca."
        }
        ("map.answer.partial_unsure", Locale::EnUs) => {
            "Partial. The map found it, but it is not sure this is the place. This is what the map found, next to the search result."
        }
        ("map.answer.ask", Locale::PtBr) => "Antes de explorar, o Mustard consultou o mapa com este pedido.",
        ("map.answer.ask", Locale::EnUs) => "Before exploring, Mustard asked the map about this request.",
        ("map.answer.instead", Locale::PtBr) => "Esta resposta vale no lugar da busca comum.",
        ("map.answer.instead", Locale::EnUs) => "This answer stands in for the plain search.",
        ("map.answer.names_only", Locale::PtBr) => {
            "Esta busca só lista nomes de arquivo ou conta, e roda como veio. A busca que mostra linhas recebe a resposta por função."
        }
        ("map.answer.names_only", Locale::EnUs) => {
            "This search only lists file names or counts, and runs as it came. A search that shows lines gets the answer by function."
        }
        ("map.answer.lines", Locale::PtBr) => {
            "Cada função vem com o começo e o fim, e as linhas achadas entre parênteses:"
        }
        ("map.answer.lines", Locale::EnUs) => {
            "Each function comes with its first and last line, and the lines found in parentheses:"
        }
        // A mesma frase, para a resposta que traz também o código de cada função.
        ("map.answer.lines_code", Locale::PtBr) => {
            "Cada função vem com o começo e o fim, as linhas achadas entre parênteses e o código numerado, quando cabe:"
        }
        ("map.answer.lines_code", Locale::EnUs) => {
            "Each function comes with its first and last line, the lines found in parentheses and its numbered code, when it fits:"
        }
        ("map.answer.map_only", Locale::PtBr) => {
            "A busca comum não acharia nenhuma linha com esse texto. O mapa aponta estes arquivos: {files}."
        }
        ("map.answer.map_only", Locale::EnUs) => {
            "The plain search would find no line with this text. The map points to these files: {files}."
        }
        ("map.answer.changed", Locale::PtBr) => "mudado nesta onda",
        ("map.answer.changed", Locale::EnUs) => "changed in this wave",
        ("map.answer.rest", Locale::PtBr) => {
            "Fora do corte, lugares: {places}, arquivos: {files}. Repita a busca para ver a lista inteira."
        }
        ("map.answer.rest", Locale::EnUs) => {
            "Left out by the cut, places: {places}, files: {files}. Repeat the search to see the whole list."
        }
        // A parcial vai ao lado do resultado da busca, que traz a lista
        // inteira; e a função cortada diz quantas linhas ficaram de fora.
        ("map.answer.rest_beside", Locale::PtBr) => {
            "Fora do corte, lugares: {places}, arquivos: {files}. O resultado da busca mostra a lista inteira."
        }
        ("map.answer.rest_beside", Locale::EnUs) => {
            "Left out by the cut, places: {places}, files: {files}. The search result shows the whole list."
        }
        ("map.answer.more_lines", Locale::PtBr) => "… (+{count} linhas)",
        ("map.answer.more_lines", Locale::EnUs) => "… (+{count} lines)",
        // A linha que a resposta cravada leva no fim: se o primeiro achado
        // não serve, as ferramentas de sempre seguem valendo.
        ("map.search.use_tools", Locale::PtBr) => {
            "Se este não for o lugar, use suas ferramentas padrões: `Grep`, `Glob` e `Read`."
        }
        ("map.search.use_tools", Locale::EnUs) => {
            "If this is not the place, use your standard tools: `Grep`, `Glob` and `Read`."
        }
        // A resposta do filtro que escolheu "nenhum destes": nada da lista é o
        // que se procura.
        ("map.search.filter_none", Locale::PtBr) => "não encontrei nada, use suas ferramentas padrões",
        ("map.search.filter_none", Locale::EnUs) => "found nothing, use your standard tools",
        ("map.search.name_piece", Locale::PtBr) => "pedaço de nome: {word}",
        ("map.search.name_piece", Locale::EnUs) => "part of a name: {word}",
        ("map.search.filter_failed", Locale::PtBr) => {
            "O filtro da busca falhou por {reason}. A resposta veio só do banco do mapa."
        }
        ("map.search.filter_failed", Locale::EnUs) => {
            "The search filter failed because of {reason}. The answer came from the map database alone."
        }
        ("map.search.reason.no_credit", Locale::PtBr) => "falta de crédito",
        ("map.search.reason.no_credit", Locale::EnUs) => "missing credit",
        ("map.search.reason.key_refused", Locale::PtBr) => "chave recusada",
        ("map.search.reason.key_refused", Locale::EnUs) => "a refused key",
        ("map.search.reason.network", Locale::PtBr) => "falha de rede",
        ("map.search.reason.network", Locale::EnUs) => "a network failure",
        ("map.search.reason.timeout", Locale::PtBr) => "tempo esgotado",
        ("map.search.reason.timeout", Locale::EnUs) => "a timeout",
        ("map.search.reason.unreadable", Locale::PtBr) => "resposta ilegível",
        ("map.search.reason.unreadable", Locale::EnUs) => "an unreadable answer",
        ("map.search.reason.too_large", Locale::PtBr) => "pedido grande demais",
        ("map.search.reason.too_large", Locale::EnUs) => "a request too large",
        ("map.search.reason.busy", Locale::PtBr) => "serviço ocupado",
        ("map.search.reason.busy", Locale::EnUs) => "a busy service",
        ("map.search.reason.refused", Locale::PtBr) => "recusa do serviço",
        ("map.search.reason.refused", Locale::EnUs) => "a refusal of the service",
        ("map.search.bad_number", Locale::PtBr) => {
            "O valor de search.{key} no mustard.json não vale, porque o número tem que ser inteiro e maior que zero. \
             A busca usa o padrão, {default}."
        }
        ("map.search.bad_number", Locale::EnUs) => {
            "The value of search.{key} in mustard.json does not count, because the number must be a whole number \
             above zero. The search uses the default, {default}."
        }
        ("map.search.bad_filter", Locale::PtBr) => {
            "O valor de search.filter no mustard.json não vale, porque o filtro tem que ser jev ou none. A busca \
             segue sem filtro."
        }
        ("map.search.bad_filter", Locale::EnUs) => {
            "The value of search.filter in mustard.json does not count, because the filter must be jev or none. The \
             search goes on without a filter."
        }
        ("map.search.missing_key", Locale::PtBr) => {
            "A busca segue sem filtro, porque falta a chave dele. Ponha a chave em jev.key no mustard.json ou na \
             variável `TYPESAFE_API_KEY`. Para não usar o filtro, ponha none em search.filter."
        }
        ("map.search.missing_key", Locale::EnUs) => {
            "The search goes on without a filter, because its key is missing. Put the key in jev.key in mustard.json \
             or in the `TYPESAFE_API_KEY` variable. To not use the filter, set search.filter to none."
        }
        ("map.search.key_in_git", Locale::PtBr) => {
            "A chave em jev.key não vale, porque o git guarda o mustard.json. Tire o arquivo do git e troque a \
             chave, porque quem lê o repositório pode tê-la visto."
        }
        ("map.search.key_in_git", Locale::EnUs) => {
            "The key in jev.key does not count, because git tracks mustard.json. Take the file out of git and \
             change the key, because anyone who reads the repository may have seen it."
        }
        ("map.history_unreadable", Locale::PtBr) => "A história de `{file}` não pôde ser lida do git ({detail}).",
        ("map.history_unreadable", Locale::EnUs) => "The history of `{file}` could not be read from git ({detail}).",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::translate;

    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas. Quem muda um texto de propósito grava aqui os dois números
    /// novos que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged(
            include_str!("map.rs"),
            super::PREFIXES,
            80,
            0x28d1_10b2_1e48_1467,
        );
    }

    /// O que a resposta de quem usa diz das ligações — o cabeçalho, o próximo
    /// passo da suspeita, a contagem do nome comum e as rotas com as chamadas
    /// da tela — e o aviso do mapa sem a história do git passam na
    /// conferência de escrita das respostas, nos dois idiomas, com cada vaga
    /// trocada por uma palavra.
    #[test]
    fn the_users_texts_read_clearly() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            for key in [
                "scan.map.pointer",
                "map.summary.ask",
                "map.search.pass",
                "map.users.head",
                "map.users.none",
                "map.users.suspect",
                "map.users.common.one",
                "map.users.common.many",
                "map.users.routes",
                "map.users.route_suspect",
                "map.history.no_base",
                "map.history.base_not_found",
                "map.history.head",
                "map.history.form",
                "map.history.next",
                "map.history.not_in_base",
                "map.history.pick_file",
                "map.history.bad_setting",
                "map.history.pull_missing",
                "map.history.spec",
                "map.history_unreadable",
                "map.changed_in_copy",
                "map.changed_in_copy_range",
                "map.search.not_found",
                "map.answer.pinned",
                "map.answer.partial",
                "map.answer.partial_unsure",
                "map.answer.ask",
                "map.answer.instead",
                "map.answer.names_only",
                "map.answer.lines",
                "map.answer.lines_code",
                "map.answer.map_only",
                "map.answer.changed",
                "map.answer.rest",
                "map.answer.rest_beside",
                "map.answer.more_lines",
                "map.search.use_tools",
                "map.search.filter_none",
                "map.search.filter_failed",
                "map.search.bad_number",
                "map.search.bad_filter",
                "map.search.missing_key",
                "map.search.key_in_git",
                "scan.bad_max_same_name",
            ] {
                let text = translate(key, lang)
                    .replace("{name}", "run")
                    .replace("{file}", "a.rs")
                    .replace("{count}", "12")
                    .replace("{base}", "dev")
                    .replace("{line}", "4")
                    .replace("{first}", "4")
                    .replace("{last}", "9")
                    .replace("{commit}", "abc")
                    .replace("{detail}", "falha")
                    .replace("{key}", "historyCommits")
                    .replace("{default}", "10")
                    .replace("{number}", "7")
                    .replace("{spec}", "entrega")
                    .replace("{code}", "combinado")
                    .replace("{sentence}", "O pedido sai em uma frase.")
                    .replace("{reason}", translate("map.search.reason.timeout", lang))
                    .replace("{words}", "\"boleto\", \"vencido\"")
                    .replace("{missing}", "\"desconto\"")
                    .replace("{places}", "9")
                    .replace("{files}", "`a.rs`, `b.rs`")
                    .replace("{next}", "grep -rniE \"boleto|vencido\" .")
                    .replace("{value}", "dois");
                let report = crate::domain::clarity::measure(&text, &[], Some(lang));
                assert!(report.passed, "{key} {lang:?}: {report:?}");
            }
        }
    }

    /// A dica do mapa diz que o `Grep`, o `grep` e o `rg` passam pelo Mustard,
    /// que responde no lugar da busca, e o texto da busca é o de sempre: não
    /// pede palavras nem frase à parte, nem a tradução delas, e não traz vaga
    /// a preencher. Passa na conferência de escrita nos dois idiomas.
    #[test]
    fn the_map_hint_says_the_usual_search_goes_through_mustard() {
        for (lang, opening, tools) in [
            (Locale::PtBr, "Procure código como sempre, com o mesmo texto", "`Grep`, `grep` e `rg` passam pelo Mustard"),
            (Locale::EnUs, "Search for code as always, with the same text", "`Grep`, `grep` and `rg` go through Mustard"),
        ] {
            let text = translate("scan.map.pointer", lang);
            assert!(text.starts_with(opening), "{lang:?}: {text}");
            assert!(text.contains(tools), "{lang:?}: {text}");
            for gone in ["--query", "--intent", "{code_language}", "{example}", "camelCase", "snake_case"] {
                assert!(!text.contains(gone), "{lang:?}: `{gone}` in {text}");
            }
            assert!(!text.contains('{'), "{lang:?}: the hint has no slot to fill: {text}");
            let report = crate::domain::clarity::measure(text, &[], Some(lang));
            assert!(report.passed, "{lang:?}: {report:?}");
        }
    }

    /// A dica do mapa diz o que cada marca da resposta quer dizer — cravado,
    /// parcial e não achou — com o nome que a resposta dá a ela, nos dois
    /// idiomas.
    #[test]
    fn the_map_hint_teaches_what_each_mark_of_the_answer_means() {
        for (lang, marks) in [
            (
                Locale::PtBr,
                [
                    "Cravado: o mapa achou pelo nome, e a resposta já traz o trecho.",
                    "Parcial: o mapa achou parte, a busca roda, e a nota diz o que falta.",
                    "Não achei: a busca comum roda, com uma linha do que o mapa não achou.",
                ],
            ),
            (
                Locale::EnUs,
                [
                    "Pinned: the map found it by name, and the answer already carries the excerpt.",
                    "Partial: the map found part, the search runs, and the note says what is missing.",
                    "Found nothing: the plain search runs, with one line of what the map did not find.",
                ],
            ),
        ] {
            let text = translate("scan.map.pointer", lang);
            for mark in marks {
                assert!(text.contains(mark), "{lang:?}: {mark} in {text}");
            }
            let answers = [translate("map.answer.pinned", lang), translate("map.answer.partial", lang)];
            let heads: Vec<&str> = answers.iter().filter_map(|answer| answer.split(['.', ':']).next()).collect();
            for head in heads {
                assert!(text.contains(&format!("{head}:")), "{lang:?}: the answer opens with `{head}`: {text}");
            }
        }
    }

    /// A resposta parcial vai ao lado do resultado da busca: nos dois idiomas,
    /// as duas frases da marca dizem que é o que o mapa achou, e nenhuma manda
    /// buscar de novo; a linha do que ficou de fora manda ver a lista no
    /// resultado da busca, não repetir a busca. A função cortada diz quantas
    /// linhas ficaram de fora.
    #[test]
    fn the_partial_answer_sits_beside_the_search_and_never_asks_to_search_again() {
        for (lang, beside, again) in [
            (Locale::PtBr, "É o que o mapa achou, ao lado do resultado da busca.", ["Busque de novo", "Repita a busca"]),
            (Locale::EnUs, "This is what the map found, next to the search result.", ["Search again", "Repeat the search"]),
        ] {
            for key in ["map.answer.partial", "map.answer.partial_unsure"] {
                let text = translate(key, lang);
                assert!(text.ends_with(beside), "{key} {lang:?}: {text}");
                assert!(again.iter().all(|phrase| !text.contains(phrase)), "{key} {lang:?}: {text}");
            }
            let rest = translate("map.answer.rest_beside", lang);
            assert!(again.iter().all(|phrase| !rest.contains(phrase)), "{lang:?}: {rest}");
            assert!(translate("map.answer.more_lines", lang).contains("{count}"), "{lang:?}");
        }
    }

    /// A dica do mapa cita o comando com o texto da busca de sempre, sem a
    /// opção das palavras nem a da frase, e manda ler os arquivos apontados
    /// depois. As três ferramentas de busca aparecem juntas, uma vez só: o
    /// `Grep` é a busca de sempre, não uma saída para depois do "não achei".
    #[test]
    fn the_map_hint_cites_the_command_with_the_usual_text() {
        for (lang, command, reading) in [
            (Locale::PtBr, "rode `mustard-rt run map search \"<padrão>\"`", "Depois leia os arquivos apontados"),
            (Locale::EnUs, "run `mustard-rt run map search \"<pattern>\"`", "Then read the files it points to"),
        ] {
            let text = translate("scan.map.pointer", lang);
            assert!(text.contains(command), "{lang:?}: {text}");
            assert!(text.contains(reading), "{lang:?}: {text}");
            assert_eq!(text.matches("`Grep`").count(), 1, "{lang:?}: {text}");
            assert!(!text.contains("`Glob`"), "{lang:?}: {text}");
        }
    }

    /// A linha de quando o mapa não achou diz que não achou e manda seguir com
    /// `Grep`, `Glob` e `Read`; a busca exata, para começar, vem depois dessa
    /// ordem, nos dois idiomas.
    #[test]
    fn the_not_found_answer_sends_the_reader_on_with_the_standard_tools() {
        for lang in [Locale::PtBr, Locale::EnUs] {
            let text = translate("map.search.not_found", lang);
            let tools = text.find("`Grep`, `Glob` and `Read`").or_else(|| text.find("`Grep`, `Glob` e `Read`"));
            let next = text.find("{next}");
            assert!(tools.is_some() && next.is_some(), "{lang:?}: {text}");
            assert!(tools < next, "{lang:?}: the tools come before the exact search: {text}");
            assert!(text.starts_with("Não achei {words}") || text.starts_with("Found nothing for {words}"), "{lang:?}: {text}");
        }
    }

    /// A recusa da declaração mudada na cópia diz, nos dois idiomas, a faixa
    /// que ela ocupa na cópia (linhas 4 a 6) e não cita a linha do mapa (3);
    /// sem a faixa, diz a linha do mapa.
    #[test]
    fn the_changed_in_copy_refusal_names_the_copy_range_when_it_has_one() {
        use crate::domain::project_map::MapRefusal;
        let refusal = |copy| MapRefusal::ChangedInCopy {
            file: "src/a.rs".to_string(),
            name: "run".to_string(),
            line: 3,
            copy,
        };
        for lang in [Locale::PtBr, Locale::EnUs] {
            let ranged = refusal(Some((4, 6))).message(lang);
            assert!(ranged.contains('4') && ranged.contains('6') && !ranged.contains('3'), "{lang:?}: {ranged}");
            assert!(ranged.contains("`run`") && ranged.contains("`src/a.rs`"), "{lang:?}: {ranged}");
            let unranged = refusal(None).message(lang);
            assert!(unranged.contains('3') && !unranged.contains('4'), "{lang:?}: {unranged}");
        }
    }

    /// The refusals and texts of the project map, and the `doctor` advisory on
    /// what the scan writes, come from the catalog in both languages, with the
    /// slots.
    #[test]
    fn i18n_translates_project_map_keys() {
        for (key, slots) in [
            ("map.missing", &[][..]),
            ("map.unreadable", &["{detail}"][..]),
            ("map.unfilled", &["{blocks}"][..]),
            ("map.unknown_file", &["{file}"][..]),
            ("map.unknown_declaration", &["{file}", "{name}"][..]),
            ("map.unknown_name", &["{name}"][..]),
            ("map.file_unreadable", &["{file}", "{detail}"][..]),
            ("map.changed_in_copy", &["{name}", "{file}", "{line}"][..]),
            ("map.changed_in_copy_range", &["{name}", "{file}", "{first}", "{last}"][..]),
            ("map.missing_argument", &["{question}", "{flag}"][..]),
            ("map.skill_unreadable", &["{path}", "{detail}"][..]),
            ("map.skill_missing_path", &["{paths}"][..]),
            ("map.skill_too_long", &["{lines}", "{max}"][..]),
            ("map.no_target", &[][..]),
            ("map.no_examples", &["{folder}"][..]),
            ("map.why.same_folder", &[][..]),
            ("map.why.near_folder", &[][..]),
            ("map.why.imports", &["{shared}", "{of}"][..]),
            ("map.why.tested", &["{tests}"][..]),
            ("map.why.inline_tests", &[][..]),
            ("map.why.recent", &["{date}"][..]),
            ("map.why.size", &["{loc}"][..]),
            ("map.summary.head", &["{files}", "{languages}"][..]),
            ("map.summary.projects", &[][..]),
            ("map.summary.project_line", &["{name}", "{dir}", "{kind}", "{files}"][..]),
            ("map.summary.hubs", &["{files}"][..]),
            ("map.summary.recent", &["{files}"][..]),
            ("map.summary.ask", &[][..]),
            ("map.users.head", &["{name}"][..]),
            ("map.users.none", &["{name}", "{file}"][..]),
            ("map.users.suspect", &[][..]),
            ("map.users.common.one", &["{name}"][..]),
            ("map.users.common.many", &["{count}", "{name}"][..]),
            ("map.users.routes", &["{name}"][..]),
            ("map.users.route_suspect", &[][..]),
            ("map.history.no_base", &[][..]),
            ("map.history.base_not_found", &["{base}"][..]),
            ("map.history.head", &["{name}", "{file}", "{line}", "{base}", "{count}"][..]),
            ("map.history.form", &[][..]),
            ("map.history.next", &["{commit}"][..]),
            ("map.history.not_in_base", &["{base}", "{name}", "{file}"][..]),
            ("map.history.pick_file", &["{name}"][..]),
            ("map.history.bad_setting", &["{key}", "{default}"][..]),
            ("map.history.pull_missing", &["{number}"][..]),
            ("map.history.spec", &["{spec}", "{code}", "{sentence}"][..]),
            ("map.history_unreadable", &["{file}", "{detail}"][..]),
            ("map.search.not_found", &["{words}", "{next}"][..]),
            ("map.search.pass", &[][..]),
            ("scan.map.pointer", &[][..]),
            ("map.answer.pinned", &["{words}"][..]),
            ("map.answer.partial", &["{missing}"][..]),
            ("map.answer.partial_unsure", &[][..]),
            ("map.answer.ask", &[][..]),
            ("map.answer.instead", &[][..]),
            ("map.answer.names_only", &[][..]),
            ("map.answer.lines", &[][..]),
            ("map.answer.lines_code", &[][..]),
            ("map.answer.map_only", &["{files}"][..]),
            ("map.answer.changed", &[][..]),
            ("map.answer.rest", &["{places}", "{files}"][..]),
            ("map.answer.rest_beside", &["{places}", "{files}"][..]),
            ("map.answer.more_lines", &["{count}"][..]),
            ("map.search.use_tools", &[][..]),
            ("map.search.filter_none", &[][..]),
            ("map.search.name_piece", &["{word}"][..]),
            ("map.search.filter_failed", &["{reason}"][..]),
            ("map.search.reason.no_credit", &[][..]),
            ("map.search.reason.key_refused", &[][..]),
            ("map.search.reason.network", &[][..]),
            ("map.search.reason.timeout", &[][..]),
            ("map.search.reason.unreadable", &[][..]),
            ("map.search.reason.too_large", &[][..]),
            ("map.search.reason.busy", &[][..]),
            ("map.search.reason.refused", &[][..]),
            ("map.search.bad_number", &["{key}", "{default}"][..]),
            ("map.search.bad_filter", &[][..]),
            ("map.search.missing_key", &[][..]),
            ("map.search.key_in_git", &[][..]),
            ("scan.bad_max_same_name", &["scan.max_same_name", "{value}", "{default}"][..]),
            ("doctor.scan_output.visible", &["{paths}"][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
    }
}
