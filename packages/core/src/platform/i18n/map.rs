//! O mapa do projeto: as recusas e os motivos do comando `map`, o resumo do
//! início da sessão, o terreno e o `scan-map`.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["map", "orient", "scan"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // Orientation artifacts — the once-per-session terrain banner
        // (`commands/orient.rs`) and the machine-owned `.claude/scan-map.md`
        // (`commands/scan_claude.rs::render_map`). Both are DISPLAYED to the
        // developer and injected into the session, so they follow the
        // project's text language (`language.text` in `mustard.json`) — unlike
        // the internal census/index/search, which stays English by policy. The
        // `{kind}` / `{count}` / `{map}` slots are interpolated by the caller.
        ("orient.terrain.header", Locale::PtBr) => {
            "[Terreno] subprojetos mapeados pelo /scan — leia daqui, não grepe para se orientar:"
        }
        ("orient.terrain.header", Locale::EnUs) => {
            "[Terrain] subprojects mapped by /scan — read from here, don't grep to orient yourself:"
        }
        ("orient.census.files_suffix", Locale::PtBr) => " · {count} arquivos",
        ("orient.census.files_suffix", Locale::EnUs) => " · {count} files",
        ("orient.census.truncated", Locale::PtBr) => {
            "\n- (+{count} subprojetos não listados — o censo completo está em `{map}`)"
        }
        ("orient.census.truncated", Locale::EnUs) => {
            "\n- (+{count} subprojects not listed — the full census is in `{map}`)"
        }
        ("scan.map.type_line", Locale::PtBr) => "Tipo: {kind} · {count} arquivos",
        ("scan.map.type_line", Locale::EnUs) => "Type: {kind} · {count} files",
        ("scan.map.pointer", Locale::PtBr) => {
            "O terreno já está na sua janela (o resumo do mapa injetado no início da sessão). Para localizar: `grep` para termo exato conhecido; `mustard-rt run map search --query \"<palavras>\"` para conceito. Depois leia os arquivos apontados: o mapa acha onde olhar, não substitui ler."
        }
        ("scan.map.pointer", Locale::EnUs) => {
            "The terrain is already in your window (the map summary injected at session start). To locate: `grep` for a known exact term; `mustard-rt run map search --query \"<words>\"` for a concept. Then read the files it points to: the map finds where to look, it does not replace reading."
        }
        // The project map (`run map`): refusals, reasons of the examples and
        // the session-start summary.
        ("map.missing", Locale::PtBr) => "O mapa do projeto ainda não existe. Rode `mustard-rt run scan`.",
        ("map.missing", Locale::EnUs) => "The project map does not exist yet. Run `mustard-rt run scan`.",
        ("map.unreadable", Locale::PtBr) => {
            "O mapa do projeto não pôde ser lido ({detail}). Rode `mustard-rt run scan` de novo."
        }
        ("map.unreadable", Locale::EnUs) => {
            "The project map could not be read ({detail}). Run `mustard-rt run scan` again."
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
        ("map.file_unreadable", Locale::PtBr) => {
            "O arquivo `{file}` está no mapa e não pôde ser lido ({detail}). Confira se ele ainda \
             está no lugar."
        }
        ("map.file_unreadable", Locale::EnUs) => {
            "The file `{file}` is in the map and could not be read ({detail}). Check whether it is \
             still there."
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
             `slice --file <caminho> --name <declaração>`, `users --name <declaração>`, \
             `history --name <declaração>` ou `search --query \"<palavras>\"`."
        }
        ("map.summary.ask", Locale::EnUs) => {
            "Ask the map: `mustard-rt run map examples --file <path>`, `importers`, `tests`, \
             `slice --file <path> --name <declaration>`, `users --name <declaration>`, \
             `history --name <declaration>` or `search --query \"<words>\"`."
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
        ("map.users.common", Locale::PtBr) => {
            "{count} chamadas de `{name}` ficaram sem ligação, porque o nome é comum demais para o mapa decidir. \
             Para achá-las, peça findReferences nesta declaração à ferramenta `LSP` (servidor de linguagem) \
             do Claude Code."
        }
        ("map.users.common", Locale::EnUs) => {
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
            46,
            0x7527_204a_6e03_e87b,
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
                "map.users.head",
                "map.users.none",
                "map.users.suspect",
                "map.users.common",
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
            ] {
                let text = translate(key, lang)
                    .replace("{name}", "run")
                    .replace("{file}", "a.rs")
                    .replace("{count}", "12")
                    .replace("{base}", "dev")
                    .replace("{line}", "4")
                    .replace("{commit}", "abc")
                    .replace("{detail}", "falha")
                    .replace("{key}", "historyCommits")
                    .replace("{default}", "10")
                    .replace("{number}", "7")
                    .replace("{spec}", "entrega")
                    .replace("{code}", "combinado")
                    .replace("{sentence}", "O pedido sai em uma frase.");
                let report = crate::domain::clarity::measure(&text, &[], Some(lang));
                assert!(report.passed, "{key} {lang:?}: {report:?}");
            }
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
            ("map.unknown_file", &["{file}"][..]),
            ("map.unknown_declaration", &["{file}", "{name}"][..]),
            ("map.file_unreadable", &["{file}", "{detail}"][..]),
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
            ("map.users.common", &["{count}", "{name}"][..]),
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
