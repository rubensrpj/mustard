//! A conferência dos restos depois da onda: o que a onda tirou do projeto
//! sai completo. Tirado é a declaração ou o arquivo que está no mapa da base
//! da rodada e não está no mapa de depois da junção.
//!
//! - Resto pelo nome: o nome tirado, como palavra inteira, nos arquivos que
//!   o git rastreia e nos que a onda criou (código, comentário, teste,
//!   documento, molde, script), com o arquivo e a linha. O nome que continua
//!   declarado noutro lugar não conta pelo texto: ali o texto pode citar a
//!   outra declaração, e o mapa de depois já liga cada chamada a ela. A spec
//!   (`.claude/spec/`), o registro de mudanças do projeto e o histórico do
//!   git ficam de fora: são registro do que aconteceu. Só se procura o nome
//!   que não se confunde com uma palavra da prosa ([`distinctive`]). O nome de
//!   um arquivo tirado, que não tem declaração, conta pelo texto em qualquer
//!   lugar, com o desconto da prosa só no comentário. O mesmo nome de uma
//!   declaração pode ser outra coisa, e cada lugar pede uma leitura
//!   ([`other_thing`]):
//!   - O arquivo do mapa da base que nesse mapa não usava a declaração tirada
//!     (nenhum uso dela aponta para o arquivo): a linha de código não conta,
//!     porque o nome ali é outra coisa, como uma variável local que o mapa
//!     não guarda, e um uso de verdade o mapa da base teria ligado; a linha de
//!     comentário só conta com a citação qualificada (`modulo::nome`,
//!     `modulo.nome`), nunca com o nome sozinho, ainda que entre crases.
//!   - O arquivo que usava a declaração, ou que a onda criou e está no mapa de
//!     depois: a linha de código conta; a de comentário conta quando o nome
//!     nela não é palavra da prosa ([`prose_comment`]): entre crases, ligado a
//!     caminho (`Modo::nome`, `modo.nome`) ou com grafia de identificador
//!     (`snake_case`, `camelCase`, com dígito); o nome todo em maiúsculas de
//!     até quatro letras (`OFF`, `ALL`), que a prosa usa como palavra, só
//!     conta entre crases.
//!   - O documento e o molde (`.md`, `.txt`, `.rst`, `.adoc`, a pasta de
//!     moldes), que o mapa não tem: o mesmo teste da prosa, sem a exigência de
//!     ser comentário ([`prose_document`]); a palavra solta, mesmo toda em
//!     maiúsculas, não conta.
//!   - O outro arquivo que o mapa não tem (script de instalação, fluxo de
//!     integração, configuração, instalador): só conta a citação qualificada
//!     por `::` (`mustard_cli::NOME`, `Tipo::nome`), porque ali a palavra é
//!     variável do próprio arquivo (`$NOME`).
//! - Órfão: a declaração que tinha uso fora de teste no mapa da base, e cujo
//!   último uso a onda tirou, e ficou sem nenhum. A de antes é a de mesmo
//!   nome, tipo e dono ([`same_piece`]): o campo de mesmo nome de outro tipo
//!   do arquivo é outra peça. Teste chamando não conta
//!   como uso: nem o arquivo de teste, nem o trecho de teste de um arquivo
//!   do programa, que o scan reconhece e o mapa guarda pelas linhas. A
//!   declaração escrita dentro desse trecho é do teste e não entra. A
//!   declaração já sem uso na base não entra, e o ponto de entrada nunca é
//!   órfão: a função principal, a que atende uma rota do
//!   mapa, o método que cumpre um contrato (chamado por quem registra o
//!   tipo) e a declarada no arquivo de entrada da pasta, que exporta o
//!   pacote. O campo, a propriedade ou o membro de enum que ainda é lido
//!   pelo texto (`objeto.nome`, `objeto->nome`) fora de teste, num arquivo
//!   que uma onda mudou ou que o lia na base, não é órfão: o mapa não liga a
//!   leitura depois de uma variável cujo tipo o arquivo não escreve
//!   ([`member_read_remains`]). O órfão cujo nome ainda aparece como palavra
//!   inteira fora de teste e fora das linhas dele, pela mesma busca do resto
//!   pelo nome, vai só como aviso: o texto pode ser um uso que o mapa não
//!   ligou.
//! - Posto sem uso: a declaração que está no mapa de depois e não está no de
//!   antes (de mesmo nome, tipo e dono, no mesmo arquivo; o arquivo que o mapa
//!   da base não tem é todo novo), no arquivo de uma onda, e que nenhum
//!   trecho do programa usa. Teste chamando não conta como uso, como no
//!   órfão, e vale o mesmo ponto de entrada livre, com o tipo que tem um
//!   método de rota, o método que cumpre um contrato (a implementação de um
//!   traço, ainda que o traço seja de fora do projeto) e o campo que o texto
//!   ainda lê. A declaração já sem uso na base, que continua no mesmo
//!   arquivo, fica de fora: não foi a onda que a pôs. O achado recusa quando
//!   o nome não está escrito noutro lugar fora de teste e só avisa quando
//!   está, pela mesma busca do órfão. O teste, de arquivo de teste ou do
//!   trecho de teste de um arquivo do programa, cujo texto não cita nenhuma
//!   outra declaração chamável do programa sai na mesma mensagem como teste
//!   sem uso, quando o achado dela recusa: o teste só existe por ela. A
//!   declaração com atributo ou decorador de teste em cima ([`test_gated`]) é
//!   do teste e não entra.

use std::cell::LazyCell;
use std::collections::BTreeSet;
use std::path::Path;

use mustard_core::domain::ast::{is_entry_file, is_test_path};
use mustard_core::domain::project_map::{MapDecl, MapModule, ProjectMap, UseSite};
use mustard_core::platform::git as git_exec;
use mustard_core::platform::i18n::{translate, Locale};

use super::commit::{AfterWave, Finding};

/// Os nomes de arquivo, sem a extensão e sem diferença de caixa, do registro
/// de mudanças de um projeto.
const CHANGE_LOGS: &[&str] = &["changelog", "changes", "history", "news", "releases", "release_notes", "release-notes"];

/// As extensões de documento de texto, as mesmas que o registro de mudanças
/// usa (sem extensão também vale para ele).
const DOCUMENT_EXTENSIONS: &[&str] = &["md", "txt", "rst", "adoc"];

/// A pasta que guarda os moldes de um projeto.
const MOLD_FOLDER: &str = "templates";

/// Os achados dos restos, dos órfãos e do código novo sem uso de cada onda de
/// `maps`, procurando o texto em `root`, já com a junção no disco.
pub(super) fn findings(root: &Path, maps: &AfterWave, lang: Locale) -> Vec<Finding> {
    let declared: BTreeSet<&str> =
        maps.after.modules.iter().flat_map(|m| &m.declarations).map(|d| d.name.as_str()).collect();
    let file_names: BTreeSet<&str> = maps.after.modules.iter().map(|m| file_name(&m.path)).collect();
    let created = created(root, maps);
    let mut searched: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for (wave, files) in &maps.changed {
        for file in files {
            let Some(before) = maps.base.module(file) else { continue };
            let now = maps.after.module(file);
            let mut gone: Vec<&str> = before
                .declarations
                .iter()
                .map(|d| d.name.as_str())
                .filter(|name| !now.is_some_and(|m| m.declarations.iter().any(|d| d.name == *name)))
                .filter(|name| !declared.contains(name) && distinctive(name))
                .collect();
            if now.is_none() && !file_names.contains(file_name(file)) {
                gone.push(file_name(file));
            }
            for name in gone.into_iter().filter(|name| searched.insert((*name).to_string())) {
                let removed: Vec<&MapDecl> = before.declarations.iter().filter(|d| d.name == name).collect();
                for (site, line, text) in cited(root, name, &created) {
                    if other_thing(maps, &removed, name, &site, &text) {
                        continue;
                    }
                    let text = translate("round.after_wave.leftover", lang)
                        .replace("{file}", &site)
                        .replace("{line}", &line.to_string())
                        .replace("{name}", name)
                        .replace("{from}", file);
                    out.push(Finding { wave: *wave, refuses: true, text });
                }
            }
        }
    }
    out.extend(dead_code(root, maps, &created, lang));
    out
}

/// As declarações sem uso fora de teste que a onda deixou: a que perdeu o
/// último uso porque a onda o tirou (órfã, com a onda que o tirou) e a que a
/// onda pôs e nada usa (posta, com a onda dona do arquivo). A que ainda tem
/// o nome escrito fora de teste ([`cited_outside_tests`]) vai só como aviso:
/// o texto pode ser um uso que o mapa não ligou.
fn dead_code(root: &Path, maps: &AfterWave, created: &[String], lang: Locale) -> Vec<Finding> {
    let called = LazyCell::new(|| called_names(maps));
    let mut out = Vec::new();
    for module in maps.after.modules.iter().filter(|m| !is_test_path(&m.path) && !is_entry_file(&m.path, &m.language)) {
        let before = maps.base.module(&module.path);
        for decl in module.declarations.iter().filter(|decl| !in_test_lines(module, decl.line)) {
            if entry_point(module, before, decl) || decl.used_by.iter().any(|site| from_program(&maps.after, site)) {
                continue;
            }
            match before.and_then(|b| b.declarations.iter().find(|d| same_piece(d, decl))) {
                Some(old) => out.extend(lost_last_use(root, maps, created, lang, (module, decl, old))),
                None => out.extend(never_used(root, maps, created, lang, (module, decl), &called)),
            }
        }
    }
    out
}

/// O ponto de entrada nunca é código sem uso: a função principal, a que
/// atende uma rota do mapa, a que cumpre um contrato (a implementação de um
/// traço ou de uma interface, chamada por quem registra o tipo) e o tipo que
/// tem um método de rota, que o servidor instancia sozinho.
fn entry_point(module: &MapModule, before: Option<&MapModule>, decl: &MapDecl) -> bool {
    let routed = |d: &MapDecl| {
        module.routes.iter().chain(before.into_iter().flat_map(|b| &b.routes)).any(|route| route.handler == d.name)
    };
    let owns_route = decl.members.iter().filter(|at| at.file == module.path).any(|at| {
        module.declarations.iter().any(|d| d.name == at.name && u64::try_from(at.line) == Ok(d.line) && routed(d))
    });
    decl.name == "main" || routed(decl) || !decl.implements.is_empty() || !decl.contract.is_empty() || owns_route
}

/// O achado da declaração `decl` de `module` (a de antes é `old`) que perdeu
/// o último uso fora de teste, com a onda que o tirou: a dona de um arquivo
/// que a usava na base.
fn lost_last_use(
    root: &Path,
    maps: &AfterWave,
    created: &[String],
    lang: Locale,
    (module, decl, old): (&MapModule, &MapDecl, &MapDecl),
) -> Option<Finding> {
    let callers: Vec<&str> =
        old.used_by.iter().filter(|site| from_program(&maps.base, site)).map(|site| site.file.as_str()).collect();
    let wave = maps.changed.iter().find(|(_, files)| files.iter().any(|f| callers.contains(&f.as_str())))?.0;
    if member_read_remains(root, maps, module, decl, &callers) {
        return None;
    }
    let text = translate("round.after_wave.orphan", lang)
        .replace("{name}", &decl.name)
        .replace("{file}", &module.path)
        .replace("{line}", &decl.line.to_string());
    let refuses = !cited_outside_tests(maps, module, decl, &cited(root, &decl.name, created));
    Some(Finding { wave, refuses, text })
}

/// Os achados da declaração `decl` de `module` que a onda pôs e nenhum
/// trecho do programa usa: o dela e o de cada teste que só existe por ela
/// ([`tests_only_for`]), todos do mesmo peso. A onda é a dona do arquivo; o
/// arquivo que nenhuma onda mudou não é dela, e o campo que o texto ainda lê
/// não conta ([`member_read_remains`]).
fn never_used(
    root: &Path,
    maps: &AfterWave,
    created: &[String],
    lang: Locale,
    (module, decl): (&MapModule, &MapDecl),
    called: &BTreeSet<&str>,
) -> Vec<Finding> {
    let Some((wave, _)) = maps.changed.iter().find(|(_, files)| files.contains(&module.path)) else { return Vec::new() };
    if test_gated(root, module, decl) || member_read_remains(root, maps, module, decl, &[]) {
        return Vec::new();
    }
    let cites = cited(root, &decl.name, created);
    let refuses = !cited_outside_tests(maps, module, decl, &cites);
    let says = |key: &str, file: &str, line: u64| Finding {
        wave: *wave,
        refuses,
        text: translate(key, lang)
            .replace("{name}", &decl.name)
            .replace("{file}", file)
            .replace("{line}", &line.to_string()),
    };
    let tests = if refuses { tests_only_for(root, maps, decl, &cites, called) } else { BTreeSet::new() };
    let mut out = vec![says("round.after_wave.unused", &module.path, decl.line)];
    out.extend(tests.iter().map(|(file, line)| says("round.after_wave.unused_test", file, *line)));
    out
}

/// Os tipos de declaração que um teste chama pelo nome: o que o mapa liga
/// por chamada ou por construção.
const CALLED_KINDS: &[&str] = &["function", "method", "class", "struct", "record", "enum_member", "constant", "const"];

/// Os nomes das declarações chamáveis do programa no mapa de depois: as de
/// fora de arquivo de teste e de fora dos trechos de teste.
fn called_names(maps: &AfterWave) -> BTreeSet<&str> {
    let program = maps.after.modules.iter().filter(|module| !is_test_path(&module.path));
    program
        .flat_map(|module| module.declarations.iter().filter(move |decl| !in_test_lines(module, decl.line)))
        .filter(|decl| CALLED_KINDS.contains(&decl.kind.as_str()))
        .map(|decl| decl.name.as_str())
        .collect()
}

/// Os testes que só existem por `decl`: cada função de teste, de arquivo de
/// teste ou de trecho de teste, que escreve o nome dela (`cites`) e não
/// escreve o de nenhuma outra declaração chamável do programa (`called`),
/// com o arquivo e a linha em que começa. O mapa não guarda a chamada escrita
/// em trecho de teste, e por isso a leitura é pelo texto da função. O teste
/// que também chama outra peça do programa não é só dela e fica de fora.
fn tests_only_for(
    root: &Path,
    maps: &AfterWave,
    decl: &MapDecl,
    cites: &[(String, usize, String)],
    called: &BTreeSet<&str>,
) -> BTreeSet<(String, u64)> {
    let mut tests = BTreeSet::new();
    for (file, line, _) in cites {
        let line = u64::try_from(*line).unwrap_or(u64::MAX);
        let in_test = is_test_path(file) || maps.after.module(file).is_some_and(|module| in_test_lines(module, line));
        if !in_test {
            continue;
        }
        let Some((first, last)) = span_around(maps, file, line) else { continue };
        let Ok(text) = std::fs::read_to_string(root.join(file)) else { continue };
        let skip = usize::try_from(first.saturating_sub(1)).unwrap_or(usize::MAX);
        let take = usize::try_from(last - first + 1).unwrap_or(usize::MAX);
        let others = text.lines().skip(skip).take(take).any(|text| {
            text.split(|c: char| !is_word_char(c)).any(|word| !word.is_empty() && word != decl.name && called.contains(word))
        });
        if !others {
            tests.insert((file.clone(), first));
        }
    }
    tests
}

/// As linhas da função ou do método de `file` que guarda a linha `line`, a
/// mais interna; `None` quando nenhuma a guarda, e então não há teste a nomear.
fn span_around(maps: &AfterWave, file: &str, line: u64) -> Option<(u64, u64)> {
    let module = maps.after.module(file)?;
    let around = module
        .declarations
        .iter()
        .filter(|d| matches!(d.kind.as_str(), "function" | "method") && d.line <= line && line <= d.end_line.max(d.line))
        .min_by_key(|d| d.end_line.max(d.line) - d.line)?;
    Some((around.line, around.end_line.max(around.line)))
}

/// A declaração `decl` de `module` é código só do teste, ainda que o mapa a
/// guarde como do programa: um atributo ou um decorador colado em cima dela
/// cita teste (`#[cfg(test)]`, `#[cfg(all(test, unix))]`, `#[test]`,
/// `[TestMethod]`, `@Test`). O scan só reconhece como trecho de teste o módulo
/// com o atributo `#[cfg(test)]` exato, e a função de apoio ou o teste escrito
/// fora dele chegaria aqui como código novo sem uso.
fn test_gated(root: &Path, module: &MapModule, decl: &MapDecl) -> bool {
    let Ok(text) = std::fs::read_to_string(root.join(&module.path)) else { return false };
    let above = usize::try_from(decl.line.saturating_sub(1)).unwrap_or(0);
    let lines: Vec<&str> = text.lines().take(above).collect();
    lines
        .iter()
        .rev()
        .map(|line| line.trim_start())
        .take_while(|line| ["#[", "#![", "@", "[", "//"].iter().any(|start| line.starts_with(start)))
        .any(|line| !line.starts_with("//") && line.to_lowercase().contains("test"))
}

/// Os tipos de declaração que se leem depois do objeto, sem chamada:
/// `pedido.total`, `cor.Vermelho`.
const READ_KINDS: &[&str] = &["field", "property", "enum_member"];

/// O campo, a propriedade ou o membro de enum `decl`, declarado em `module`,
/// ainda é lido pelo texto fora de teste: alguma linha de código escreve
/// `objeto.nome` ou `objeto->nome` (o nome como palavra inteira) num arquivo
/// que uma onda mudou ou num dos que o liam na base (`callers`). O mapa liga
/// a leitura ao campo pelo tipo que o arquivo escreve para o objeto; depois
/// de uma variável cujo tipo vem de uma chamada (`let walked = Walked::of(x)`)
/// ele não liga, e o campo parece sem uso ainda que o texto o leia. Fora
/// deste texto ficam o arquivo de teste, o trecho de teste do arquivo, a
/// linha de comentário e as linhas da própria declaração.
fn member_read_remains(root: &Path, maps: &AfterWave, module: &MapModule, decl: &MapDecl, callers: &[&str]) -> bool {
    if !READ_KINDS.contains(&decl.kind.as_str()) {
        return false;
    }
    let own = decl.line..=decl.end_line.max(decl.line);
    let files: BTreeSet<&str> =
        maps.changed.iter().flat_map(|(_, files)| files).map(String::as_str).chain(callers.iter().copied()).collect();
    files.into_iter().filter(|file| !is_test_path(file)).any(|file| {
        let Ok(text) = std::fs::read_to_string(root.join(file)) else { return false };
        let tests = maps.after.module(file).or_else(|| maps.base.module(file));
        text.lines().enumerate().any(|(at, line)| {
            let number = u64::try_from(at + 1).unwrap_or(u64::MAX);
            let own_lines = file == module.path && own.contains(&number);
            let test_block = tests.is_some_and(|site| in_test_lines(site, number));
            !own_lines && !test_block && !is_comment_line(line) && reads_member(line, &decl.name)
        })
    })
}

/// A linha só tem comentário, de linha ou de bloco, pelo começo dela.
fn is_comment_line(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("//") || line.starts_with("/*") || line.starts_with('*') || line.starts_with('#')
}

/// A linha escreve `objeto.name` ou `objeto->name`, com `name` como palavra
/// inteira. O `..name` de uma faixa e o `...name` de um espalhamento não são
/// leitura de membro.
fn reads_member(line: &str, name: &str) -> bool {
    word_starts(line, name).any(|at| {
        let before = &line[..at];
        (before.ends_with('.') && !before.ends_with("..")) || before.ends_with("->")
    })
}

/// O caractere que forma palavra: letra, dígito ou `_`.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Onde `name` aparece em `line` como palavra inteira: o byte em que cada
/// ocorrência começa. É a mesma palavra inteira que o `git grep -w` acha.
fn word_starts<'a>(line: &'a str, name: &'a str) -> impl Iterator<Item = usize> + 'a {
    line.match_indices(name).filter_map(move |(at, _)| {
        let word_before = line[..at].chars().next_back().is_some_and(is_word_char);
        let word_after = line[at + name.len()..].starts_with(is_word_char);
        (!word_before && !word_after).then_some(at)
    })
}

/// `before` e `now` são a mesma peça em dois mapas: o mesmo nome, o mesmo tipo
/// de declaração e o mesmo dono mais interno. O campo `total` de `Pedido` não
/// é o de `Nota`, ainda que os dois morem no mesmo arquivo: quem tinha uso na
/// base é a peça de mesmo dono, e uma peça nova, ou sem uso antes, não vira
/// órfã por causa da homônima de outro tipo.
fn same_piece(before: &MapDecl, now: &MapDecl) -> bool {
    before.name == now.name && before.kind == now.kind && before.owner.first() == now.owner.first()
}

/// O uso `site` é do programa, pelo mapa `map` em que ele está: fora de
/// arquivo de teste e fora dos trechos de teste do arquivo dele.
fn from_program(map: &ProjectMap, site: &UseSite) -> bool {
    let line = u64::try_from(site.line).unwrap_or(u64::MAX);
    !is_test_path(&site.file) && !map.module(&site.file).is_some_and(|module| in_test_lines(module, line))
}

/// A linha `line` cai dentro de um trecho de teste do arquivo `module`, pelas
/// linhas que o mapa guarda de cada um.
fn in_test_lines(module: &MapModule, line: u64) -> bool {
    module.test_lines.iter().any(|&(first, last)| (first..=last).contains(&line))
}

/// A linha `text` de `file` cita o nome `name` de uma declaração tirada
/// (`removed`, as de antes) e não é um uso dela: o mesmo nome é outra coisa, e
/// a citação não conta como resto. Cada lugar pede uma leitura:
///
/// - O arquivo que o mapa não tem, nem o de antes nem o de depois: o documento
///   e o molde seguem a prosa ([`prose_document`]); o script, o fluxo de
///   integração e a configuração só contam com a citação qualificada por `::`,
///   porque ali o nome é variável do próprio arquivo.
/// - O arquivo do mapa da base que nele não usava nenhuma das declarações
///   tiradas: a linha de código é outra coisa, como uma variável local que o
///   mapa não guarda (um uso de verdade o mapa da base teria ligado), e a de
///   comentário só conta com a citação qualificada, além do teste da prosa.
/// - O arquivo que usava a declaração, ou que a onda criou e o mapa de depois
///   tem: a linha de código conta, e a de comentário passa pelo teste da
///   prosa ([`prose_comment`]).
///
/// Sem declaração de antes (o nome de um arquivo tirado) só vale o teste da
/// prosa, que só olha comentário.
fn other_thing(maps: &AfterWave, removed: &[&MapDecl], name: &str, file: &str, text: &str) -> bool {
    if removed.is_empty() {
        return prose_comment(name, text);
    }
    let in_base = maps.base.module(file).is_some();
    if !in_base && maps.after.module(file).is_none() {
        return if is_document(file) { prose_document(name, text) } else { !linked_by(name, text, scoped) };
    }
    let used = removed.iter().any(|decl| decl.used_by.iter().any(|site| site.file == file));
    if in_base && !used {
        return !is_comment_line(text) || prose_comment(name, text) || !linked_by(name, text, path_linked);
    }
    prose_comment(name, text)
}

/// `path` é documento de texto ou molde: a extensão é de documento
/// ([`DOCUMENT_EXTENSIONS`]) ou o arquivo mora na pasta de moldes
/// ([`MOLD_FOLDER`]), em qualquer profundidade.
fn is_document(path: &str) -> bool {
    let lower = path.to_lowercase();
    let (folders, name) = lower.rsplit_once('/').unwrap_or(("", lower.as_str()));
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    DOCUMENT_EXTENSIONS.contains(&extension) || folders.split('/').any(|folder| folder == MOLD_FOLDER)
}

/// `link` aprova alguma ocorrência de `name` em `text`, que o recebe com o
/// que vem antes do nome na linha e o que vem depois.
fn linked_by(name: &str, text: &str, link: impl Fn(&str, &str) -> bool) -> bool {
    word_starts(text, name).any(|at| link(&text[..at], &text[at + name.len()..]))
}

/// O nome, com `before` antes dele na linha e `after` depois, está qualificado
/// por `::`: `modulo::nome`, `Tipo::nome`, `nome::novo`. Fora da linguagem que
/// escreve assim, o `.` não serve: num script ele separa a extensão
/// (`$NOME.tar.gz`) ou o campo (`env.NOME`).
fn scoped(before: &str, after: &str) -> bool {
    before.ends_with("::") || after.starts_with("::")
}

/// O nome de `decl`, declarada em `module`, aparece como palavra inteira
/// num arquivo que o git rastreia ou que a onda criou (`cites`, de
/// [`cited`]), fora de arquivo de teste, fora dos trechos de teste de cada
/// arquivo no mapa de depois e fora das linhas da própria declaração.
fn cited_outside_tests(maps: &AfterWave, module: &MapModule, decl: &MapDecl, cites: &[(String, usize, String)]) -> bool {
    let own = decl.line..=decl.end_line.max(decl.line);
    cites.iter().any(|(file, line, _)| {
        let line = u64::try_from(*line).unwrap_or(u64::MAX);
        let own_lines = *file == module.path && own.contains(&line);
        let test_block = maps.after.module(file).is_some_and(|site| in_test_lines(site, line));
        let skipped = is_test_path(file) || own_lines || test_block;
        !skipped
    })
}

/// O nome de arquivo de `path`, sem as pastas.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// O nome que se procura como texto: o que não se confunde com uma palavra
/// comum da prosa — com `_`, `.`, `-` ou dígito, ou com maiúscula depois da
/// primeira letra (`camelCase`, `PascalCase` de duas partes, `CONSTANTE`). A
/// palavra solta (`total`, `Report`) aparece em todo comentário, e o uso
/// dela no código a compilação já recusa. A `CONSTANTE` curta (`OFF`) ainda
/// é palavra da prosa em inglês: no comentário, [`prose_comment`] a separa.
fn distinctive(name: &str) -> bool {
    name.chars().count() >= 3 && unusual_spelling(name)
}

/// O nome tem grafia que a palavra comum não tem: `_`, `.`, `-`, dígito ou
/// maiúscula depois da primeira letra.
fn unusual_spelling(name: &str) -> bool {
    identifier_spelling(name) || name.chars().skip(1).any(char::is_uppercase)
}

/// O nome tem grafia de identificador, que a palavra da prosa não tem, nem
/// escrita toda em maiúsculas: `_`, `.`, `-`, dígito, ou maiúscula depois da
/// primeira letra num nome que também tem minúscula (`camelCase`,
/// `PascalCase` de duas partes). `VERSION` é palavra; `MAX_SIZE` não.
fn identifier_spelling(name: &str) -> bool {
    name.contains(['_', '.', '-'])
        || name.chars().any(|c| c.is_ascii_digit())
        || (name.chars().any(char::is_lowercase) && name.chars().skip(1).any(char::is_uppercase))
}

/// O nome todo em maiúsculas, de até quatro letras (`OFF`, `ALL`, `NONE`): a
/// prosa em inglês escreve assim a palavra, e o nome de uma constante não se
/// distingue dela.
fn shout_word(name: &str) -> bool {
    name.chars().count() <= 4 && name.chars().all(char::is_uppercase)
}

/// A linha de comentário `text` só tem `name` como palavra da prosa: nenhuma
/// ocorrência dele está entre crases, e nenhuma tem grafia de identificador
/// ([`unusual_spelling`]) ou liga a um caminho (`Modo::nome`, `modo.nome`,
/// mas não o ponto final da frase). O nome em maiúsculas curto ([`shout_word`])
/// só conta entre crases. A linha de código não é prosa: o uso dela segue
/// contando.
fn prose_comment(name: &str, text: &str) -> bool {
    is_comment_line(text)
        && !word_starts(text, name).any(|at| {
            let (before, after) = (&text[..at], &text[at + name.len()..]);
            in_ticks(before) || (!shout_word(name) && (unusual_spelling(name) || path_linked(before, after)))
        })
}

/// A linha `text` de um documento ou de um molde só tem `name` como palavra
/// da prosa: o mesmo teste de [`prose_comment`], sem a exigência de ser
/// comentário (todo o texto do documento é prosa) e com a grafia de
/// identificador ([`identifier_spelling`]) no lugar da grafia incomum: a
/// palavra solta toda em maiúsculas (`VERSION`) é palavra da prosa, e só conta
/// entre crases ou ligada a caminho.
fn prose_document(name: &str, text: &str) -> bool {
    !word_starts(text, name).any(|at| {
        let (before, after) = (&text[..at], &text[at + name.len()..]);
        in_ticks(before) || (!shout_word(name) && (identifier_spelling(name) || path_linked(before, after)))
    })
}

/// O que vem antes do nome na linha, `before`, deixa uma crase aberta: o nome
/// está escrito como código.
fn in_ticks(before: &str) -> bool {
    before.matches('`').count() % 2 == 1
}

/// O nome, com `before` antes dele na linha e `after` depois, está ligado a
/// um caminho: `Modo::nome`, `modo.nome`, `nome::novo`, `nome.campo`. O ponto
/// que fecha a frase não liga.
fn path_linked(before: &str, after: &str) -> bool {
    let reached = before.ends_with("::")
        || before
            .strip_suffix('.')
            .and_then(|head| head.chars().next_back())
            .is_some_and(|c| is_word_char(c) || c == ')' || c == ']');
    let leads = after.starts_with("::") || after.strip_prefix('.').is_some_and(|rest| rest.starts_with(is_word_char));
    reached || leads
}

/// Os arquivos que as ondas de `maps` criaram e o git ainda não rastreia,
/// fora os que ele ignora.
fn created(root: &Path, maps: &AfterWave) -> Vec<String> {
    let mut args = vec!["-c", "core.quotePath=false", "ls-files", "-z", "--others", "--exclude-standard", "--"];
    args.extend(maps.changed.iter().flat_map(|(_, files)| files).map(String::as_str));
    let out = git_exec::run(root, &args);
    out.stdout.split('\0').filter(|file| !file.is_empty()).map(str::to_string).collect()
}

/// Cada lugar que cita `name` como palavra inteira, com o arquivo, a linha e
/// o texto dela: nos arquivos que o git rastreia em `root` e nos criados
/// (`created`). A spec e o registro de mudanças ficam de fora.
fn cited(root: &Path, name: &str, created: &[String]) -> Vec<(String, usize, String)> {
    fn grep<'a>(name: &'a str, untracked: &[&'a str], paths: &[&'a str]) -> Vec<&'a str> {
        let head = ["-c", "core.quotePath=false", "grep"];
        [&head[..], untracked, &["-I", "-n", "-z", "-w", "-F", "-e", name, "--"], paths].concat()
    }
    let mut searches = vec![grep(name, &[], &[".", ":(exclude).claude/spec"])];
    if !created.is_empty() {
        let files: Vec<&str> = created.iter().map(String::as_str).collect();
        searches.push(grep(name, &["--untracked"], &files));
    }
    let mut found = BTreeSet::new();
    for args in searches {
        for line in git_exec::run(root, &args).stdout.lines() {
            let mut parts = line.splitn(3, '\0');
            let (Some(file), Some(Ok(at))) = (parts.next(), parts.next().map(str::parse)) else { continue };
            if !is_change_log(file) {
                found.insert((file.to_string(), at, parts.next().unwrap_or_default().to_string()));
            }
        }
    }
    found.into_iter().collect()
}

/// `path` é o registro de mudanças do projeto (`CHANGELOG.md`, `NEWS`…)?
fn is_change_log(path: &str) -> bool {
    let name = file_name(path).to_lowercase();
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name.as_str(), ""));
    CHANGE_LOGS.contains(&stem) && (extension.is_empty() || DOCUMENT_EXTENSIONS.contains(&extension))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::super::imports_check::tests::mine_giving;
    use super::super::tests::{approved, delivered, git_at, git_text, round, round_with_mine, write};
    use super::*;

    /// Um arquivo do mapa, com as declarações `decls` (nome, linha e quem a
    /// usa, como o scan grava: `arquivo:linha:quem`).
    fn module(path: &str, decls: &[(&str, u64, &[&str])]) -> Value {
        let declarations: Vec<Value> = decls
            .iter()
            .map(|(name, line, used)| json!({"kind": "function", "name": name, "line": line, "end_line": line + 2, "used_by": used}))
            .collect();
        json!({"path": path, "language": "rust", "declarations": declarations})
    }

    /// Um arquivo do mapa só com campos: cada um é `(dono, nome, linha, quem o
    /// usa)`, como o scan grava.
    fn fields_module(path: &str, fields: &[(&str, &str, u64, &[&str])]) -> Value {
        let declarations: Vec<Value> = fields
            .iter()
            .map(|(owner, name, line, used)| {
                json!({"kind": "field", "name": name, "line": line, "end_line": line, "used_by": used, "owner": [owner]})
            })
            .collect();
        json!({"path": path, "language": "rust", "declarations": declarations})
    }

    /// Uma spec aprovada com a onda 1, que muda `src/a.rs`; os arquivos
    /// `files` comitados no projeto; a onda já enviada; e o mapa da base
    /// `base`.
    fn project(root: &Path, files: &[(&str, &str)], base: &Value) {
        project_at(root, "src/a.rs", files, base);
    }

    /// Como [`project`], com a onda 1 mudando o arquivo `changed`.
    fn project_at(root: &Path, changed: &str, files: &[(&str, &str)], base: &Value) {
        approved(root, "x", &[(1, &[changed], &[])]);
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        git_at(root, &["add", "-A", "--", ".", ":(exclude).claude"]);
        git_at(root, &["commit", "-q", "-m", "arquivos"]);
        round(root, "x", None);
        // O mapa da base entra depois do envio, como na conferência das
        // importações.
        mustard_core::io::project_map::write_text(root, &base.to_string()).unwrap();
    }

    /// A volta da onda 1 mudando `src/a.rs`, com o mapa de depois `after`.
    fn back(root: &Path, after: Value) -> Value {
        back_at(root, "src/a.rs", after)
    }

    /// Como [`back`], com a onda 1 mudando o arquivo `changed`.
    fn back_at(root: &Path, changed: &str, after: Value) -> Value {
        let report = delivered(root, 1, "A soma mudou.", &[changed]);
        round_with_mine(root, "x", Some(&report), &mine_giving(after))
    }

    /// A resposta não traz texto nenhum da conferência depois da onda.
    fn silent(out: &Value) {
        assert_eq!(out["ok"], json!(true), "{out}");
        let text = out.to_string();
        for said in ["round-after-wave", "conferência depois da onda", "ainda cita", "sem uso fora de teste"] {
            assert!(!text.contains(said), "{said}: {out}");
        }
    }

    #[test]
    fn removing_a_function_but_leaving_a_comment_that_cites_it_gets_the_fix() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let files = [("src/b.rs", "fn outra() {}\n// soma pelo old_total antes de gravar\n")];
        let base = json!({"modules": [module("src/a.rs", &[("old_total", 1, &["src/b.rs:1:outra"]), ("keep_sum", 5, &[])]), module("src/b.rs", &[("outra", 1, &[])])]});
        project(root, &files, &base);
        let head = git_text(root, &["rev-parse", "HEAD"]);
        let after = json!({"modules": [module("src/a.rs", &[("keep_sum", 1, &[])]), module("src/b.rs", &[("outra", 1, &[])])]});
        let out = back(root, after);
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`src/b.rs` linha 2 ainda cita `old_total`, que a onda tirou de `src/a.rs`"), "{hint}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head, "nothing committed: {out}");
    }

    /// A base com `name` em `src/a.rs` e com `src/b.rs` e `src/c.rs` no mapa;
    /// `c_uses` são as chamadas de `name` que o mapa guarda (nenhuma quando
    /// vazio).
    fn base_with(name: &str, c_uses: &[&str]) -> Value {
        json!({"modules": [
            module("src/a.rs", &[(name, 1, c_uses), ("keep_sum", 5, &[])]),
            module("src/b.rs", &[("outra", 1, &[])]),
            module("src/c.rs", &[("caller", 1, &[])]),
        ]})
    }

    /// A base com `old_total` em `src/a.rs`, sem uso, e com `src/b.rs` e
    /// `src/c.rs` no mapa.
    fn base_with_old_total() -> Value {
        base_with("old_total", &[])
    }

    /// A volta da onda que tira de `src/a.rs` a declaração de [`base_with`],
    /// qualquer que seja o nome dela: o mapa de depois não a tem.
    fn back_without_old_total(root: &Path) -> Value {
        let after = json!({"modules": [
            module("src/a.rs", &[("keep_sum", 1, &[])]),
            module("src/b.rs", &[("outra", 1, &[])]),
            module("src/c.rs", &[("caller", 1, &[])]),
        ]});
        back(root, after)
    }

    #[test]
    fn a_code_line_of_a_file_whose_base_map_never_used_the_removed_function_is_another_name() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // `src/b.rs` está no mapa da base e nenhuma chamada dele ligava
        // `old_total`: a variável local de mesmo nome não é resto.
        let files = [("src/b.rs", "fn outra() {\n    for old_total in [1, 2] {}\n}\n"), ("src/c.rs", "fn caller() {}\n")];
        project(root, &files, &base_with_old_total());
        silent(&back_without_old_total(root));
    }

    #[test]
    fn a_code_line_of_a_file_the_wave_created_still_counts_by_its_text() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // `src/d.rs` o mapa da base não tem, e o de depois tem: o arquivo que a
        // onda criou segue valendo pelo texto, como o documento.
        let files = [
            ("src/b.rs", "fn outra() {}\n"),
            ("src/c.rs", "fn caller() {}\n"),
            ("src/d.rs", "fn late() {\n    for old_total in [1, 2] {}\n}\n"),
            ("docs/guide.md", "Chame old_total(x) para somar.\n"),
        ];
        project(root, &files, &base_with_old_total());
        let after = json!({"modules": [
            module("src/a.rs", &[("keep_sum", 1, &[])]),
            module("src/b.rs", &[("outra", 1, &[])]),
            module("src/c.rs", &[("caller", 1, &[])]),
            module("src/d.rs", &[("late", 1, &[])]),
        ]});
        let out = back(root, after);
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`src/d.rs` linha 2 ainda cita `old_total`, que a onda tirou de `src/a.rs`"), "{hint}");
        assert!(hint.contains("`docs/guide.md` linha 1 ainda cita `old_total`, que a onda tirou de `src/a.rs`"), "{hint}");
    }

    /// O texto de conserto da volta da onda que tira o nome `name` de
    /// `src/a.rs`, com `files` no projeto e `uses` como as chamadas que o mapa
    /// da base guarda; vazio quando a volta passa sem achado.
    fn leftovers_after_removing(name: &str, uses: &[&str], files: &[(&str, &str)]) -> String {
        let dir = tempdir().unwrap();
        let root = dir.path();
        project(root, files, &base_with(name, uses));
        let out = back_without_old_total(root);
        if out["ok"] == json!(true) {
            return String::new();
        }
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        out["hint"].as_str().unwrap_or_default().to_string()
    }

    #[test]
    fn a_script_or_config_that_has_the_removed_name_as_its_own_variable_is_not_a_leftover() {
        let files = [
            ("src/b.rs", "fn outra() {}\n"),
            ("src/c.rs", "fn caller() {}\n"),
            ("packaging/install.sh", "#!/bin/sh\nVERSION=\"${1:-latest}\"\necho \"v$VERSION\"\n"),
            ("packaging/build.ps1", "$VERSION = '1.0'\nWrite-Host $VERSION\n"),
            ("packaging/mustard.nsi", "!define VERSION \"1.0\"\n"),
            (".github/workflows/release.yml", "env:\n  VERSION: ${{ github.ref_name }}\n"),
        ];
        assert_eq!(leftovers_after_removing("VERSION", &[], &files), "");
    }

    #[test]
    fn a_script_or_config_that_qualifies_the_removed_name_by_its_path_is_a_leftover() {
        let files = [
            ("src/b.rs", "fn outra() {}\n"),
            ("src/c.rs", "fn caller() {}\n"),
            ("packaging/install.sh", "#!/bin/sh\necho \"v$VERSION\"\n"),
            (".github/workflows/release.yml", "steps:\n  - run: cargo run -- mustard_cli::VERSION\n"),
        ];
        let hint = leftovers_after_removing("VERSION", &[], &files);
        assert!(hint.contains("`.github/workflows/release.yml` linha 2 ainda cita `VERSION`, que a onda tirou de `src/a.rs`"), "{hint}");
        assert!(!hint.contains("packaging/install.sh"), "{hint}");
    }

    #[test]
    fn a_script_that_names_the_removed_file_is_still_a_leftover() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let files = [("src/b.rs", "fn outra() {}\n"), ("src/c.rs", "fn caller() {}\n"), ("packaging/build.sh", "cp src/a.rs out/\n")];
        project(root, &files, &base_with_old_total());
        // A onda tirou o arquivo `src/a.rs` inteiro.
        let after = json!({"modules": [module("src/b.rs", &[("outra", 1, &[])]), module("src/c.rs", &[("caller", 1, &[])])]});
        let out = back(root, after);
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`packaging/build.sh` linha 1 ainda cita `a.rs`, que a onda tirou de `src/a.rs`"), "{hint}");
    }

    #[test]
    fn a_comment_of_a_file_that_never_used_the_removed_name_counts_only_when_qualified() {
        // Os dois arquivos estão no mapa da base e nenhum usava `remove_dir`;
        // o comentário do primeiro fala do nome como exemplo, o do segundo
        // aponta o módulo dele.
        let files = [
            ("src/b.rs", "fn outra() {}\n// the example of the test builds `remove_dir` by hand\n"),
            ("src/c.rs", "fn caller() {}\n// the example of the test calls fs::remove_dir by hand\n"),
        ];
        let hint = leftovers_after_removing("remove_dir", &[], &files);
        assert!(hint.contains("`src/c.rs` linha 2 ainda cita `remove_dir`, que a onda tirou de `src/a.rs`"), "{hint}");
        assert!(!hint.contains("`src/b.rs`"), "{hint}");
    }

    #[test]
    fn a_comment_of_a_file_that_used_the_removed_name_still_counts_between_backticks() {
        let files = [
            ("src/b.rs", "fn outra() {}\n// the example of the test builds `remove_dir` by hand\n"),
            ("src/c.rs", "fn caller() {}\n"),
        ];
        let hint = leftovers_after_removing("remove_dir", &["src/b.rs:1:outra"], &files);
        assert!(hint.contains("`src/b.rs` linha 2 ainda cita `remove_dir`, que a onda tirou de `src/a.rs`"), "{hint}");
    }

    #[test]
    fn a_document_counts_the_removed_name_between_backticks_and_not_as_a_loose_word() {
        let files = [
            ("src/b.rs", "fn outra() {}\n"),
            ("src/c.rs", "fn caller() {}\n"),
            ("docs/loose.md", "Bump the VERSION before the release.\n"),
            ("docs/coded.md", "Bump the `VERSION` before the release.\n"),
        ];
        let hint = leftovers_after_removing("VERSION", &[], &files);
        assert!(hint.contains("`docs/coded.md` linha 1 ainda cita `VERSION`, que a onda tirou de `src/a.rs`"), "{hint}");
        assert!(!hint.contains("docs/loose.md"), "{hint}");
    }

    #[test]
    fn a_document_or_mold_counts_a_removed_name_written_as_code_and_a_plain_file_does_not() {
        let files = [
            ("src/b.rs", "fn outra() {}\n"),
            ("src/c.rs", "fn caller() {}\n"),
            ("docs/guide.md", "Call `remove_dir` to clean up.\n"),
            ("packages/core/templates/pages/spec.html", "<p>Call `remove_dir` to clean up.</p>\n"),
            ("assets/page.html", "<p>Call `remove_dir` to clean up.</p>\n"),
        ];
        let hint = leftovers_after_removing("remove_dir", &[], &files);
        assert!(hint.contains("`docs/guide.md` linha 1 ainda cita `remove_dir`"), "{hint}");
        assert!(hint.contains("`packages/core/templates/pages/spec.html` linha 1 ainda cita `remove_dir`"), "{hint}");
        assert!(!hint.contains("assets/page.html"), "{hint}");
    }

    #[test]
    fn a_document_line_counts_only_when_the_name_is_not_a_word_of_the_prose() {
        // (nome, linha, a linha só tem a palavra da prosa)
        let cases = [
            ("VERSION", "Bump the VERSION first", true),
            ("VERSION", "Bump the VERSION.", true),
            ("VERSION", "Bump `VERSION` first", false),
            ("VERSION", "Bump the mustard_cli::VERSION const", false),
            ("VERSION", "Read the VERSION.txt file", false),
            ("OFF", "The Mode::OFF state", true),
            ("OFF", "The `OFF` state", false),
            ("old_total", "Call old_total(x) to sum", false),
            ("oldTotal", "Call oldTotal to sum", false),
            ("Api2", "Uses Api2 here", false),
            ("Dockerfile", "See the Dockerfile", true),
        ];
        for (name, text, prose) in cases {
            assert_eq!(prose_document(name, text), prose, "{name} em {text:?}");
        }
    }

    #[test]
    fn only_a_text_document_or_a_file_of_the_mold_folder_is_a_document() {
        let cases = [
            ("README.md", true),
            ("docs/guide.TXT", true),
            ("docs/a.rst", true),
            ("docs/b.adoc", true),
            ("packages/core/templates/agents/wave.md", true),
            ("templates/pages/spec.html", true),
            ("Templates/settings.json", true),
            ("packaging/install.sh", false),
            (".github/workflows/release.yml", false),
            ("packaging/mustard.nsi", false),
            ("Makefile", false),
            ("src/templates.rs", false),
            ("assets/page.html", false),
        ];
        for (path, document) in cases {
            assert_eq!(is_document(path), document, "{path}");
        }
    }

    #[test]
    fn a_common_word_in_the_prose_of_a_comment_is_not_a_leftover_of_the_removed_constant() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // `OFF` saiu de `src/a.rs`, e dois comentários o usam como a palavra
        // inglesa, um deles no fim da frase.
        let prose = "fn outra() {}\n// the plugin is switched OFF when idle\n/// the flag stays OFF.\n";
        let files = [("src/b.rs", prose), ("src/c.rs", "fn caller() {}\n")];
        project(root, &files, &base_with("OFF", &[]));
        silent(&back_without_old_total(root));
    }

    #[test]
    fn a_removed_constant_between_backticks_in_a_comment_is_still_a_leftover() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let files = [("src/b.rs", "fn outra() {}\n// the plugin is switched `OFF` when idle\n"), ("src/c.rs", "fn caller() {}\n")];
        project(root, &files, &base_with("OFF", &["src/b.rs:1:outra"]));
        let head = git_text(root, &["rev-parse", "HEAD"]);
        let out = back_without_old_total(root);
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`src/b.rs` linha 2 ainda cita `OFF`, que a onda tirou de `src/a.rs`"), "{hint}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head, "nothing committed: {out}");
    }

    #[test]
    fn a_code_line_using_the_removed_constant_is_still_a_leftover_beside_a_prose_comment() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // O código de `src/c.rs` usava `OFF` na base; o comentário de
        // `src/b.rs` é só a palavra inglesa.
        let files = [
            ("src/b.rs", "fn outra() {}\n// the plugin is switched OFF when idle\n"),
            ("src/c.rs", "fn caller() {\n    let mode = OFF;\n}\n"),
        ];
        project(root, &files, &base_with("OFF", &["src/c.rs:2:caller"]));
        let head = git_text(root, &["rev-parse", "HEAD"]);
        let out = back_without_old_total(root);
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`src/c.rs` linha 2 ainda cita `OFF`, que a onda tirou de `src/a.rs`"), "{hint}");
        assert!(!hint.contains("`src/b.rs`"), "{hint}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head, "nothing committed: {out}");
    }

    #[test]
    fn a_comment_line_counts_only_when_the_name_is_not_a_word_of_the_prose() {
        // (nome, linha, a linha só tem a palavra da prosa)
        let cases = [
            ("OFF", "// switched OFF", true),
            ("OFF", "// switched OFF.", true),
            ("OFF", "// switched `OFF`", false),
            ("OFF", "// the `Mode::OFF` state", false),
            ("OFF", "// the Mode::OFF state", true),
            ("OFF", "// `ON` and OFF", true),
            ("OFF", "// OFF and `ON`", true),
            ("OFF", "    let mode = OFF;", false),
            ("OFF", "    let mode = OFF; // OFF again", false),
            ("ALL", "# ALL of them", true),
            ("NONE", "/// NONE of them", true),
            ("LIMIT", "// the LIMIT here", false),
            ("old_total", "// soma pelo old_total", false),
            ("oldTotal", "// soma pelo oldTotal", false),
            ("Api2", "// uses Api2 here", false),
            ("Dockerfile", "// see Dockerfile", true),
            ("Dockerfile", "// see Dockerfile.", true),
            ("Dockerfile", "// see ...Dockerfile", true),
            ("Dockerfile", "// see `Dockerfile`", false),
            ("Dockerfile", "// see Dockerfile.dev", false),
            ("Dockerfile", "// see build.Dockerfile", false),
            ("Dockerfile", "// see Dockerfile::run", false),
        ];
        for (name, text, prose) in cases {
            assert_eq!(prose_comment(name, text), prose, "{name} em {text:?}");
        }
    }

    #[test]
    fn removing_the_only_caller_of_a_tested_function_makes_it_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let before = json!({"modules": [
            module("src/a.rs", &[("run_sum", 1, &[])]),
            module("src/lib_sum.rs", &[("compute_total", 5, &["src/a.rs:2:run_sum", "tests/lib_sum_test.rs:3:checks"])]),
        ]});
        project(root, &[("src/lib_sum.rs", "\n\n\n\nfn compute_total() {}\n")], &before);
        let after = json!({"modules": [
            module("src/a.rs", &[("run_sum", 1, &[])]),
            module("src/lib_sum.rs", &[("compute_total", 5, &["tests/lib_sum_test.rs:3:checks"])]),
        ]});
        let out = back(root, after);
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`compute_total` em `src/lib_sum.rs` linha 5 ficou sem uso fora de teste"), "{hint}");
    }

    #[test]
    fn a_new_field_named_like_a_used_field_of_another_type_of_the_file_is_new_code_and_not_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Antes, só `Keeping` e `Finding` existem, com leitor; a onda mexe em
        // quem os lê. Depois, `Kept` nasce com campos de mesmo nome e sem
        // leitor no mapa, e a lista de tarefas ganha o `files` de outro tipo.
        // Os campos novos são código novo sem uso, e nenhum deles é órfão do
        // campo de mesmo nome que tinha leitor.
        let base = json!({"modules": [fields_module(
            "src/a.rs",
            &[
                ("Keeping", "wave", 5, &["src/a.rs:40:read"]),
                ("Keeping", "copy", 6, &["src/a.rs:41:show"]),
                ("Finding", "wave", 9, &["src/a.rs:42:report"]),
                ("Pending", "files", 12, &["src/a.rs:43:list"]),
            ],
        )]});
        project(root, &[("src/a.rs", "struct Keeping {}\n")], &base);
        let after = json!({"modules": [fields_module(
            "src/a.rs",
            &[
                ("Kept", "wave", 3, &[]),
                ("Kept", "copy", 4, &[]),
                ("Keeping", "wave", 5, &["src/a.rs:40:read"]),
                ("Keeping", "copy", 6, &["src/a.rs:41:show"]),
                ("Finding", "wave", 9, &["src/a.rs:42:report"]),
                ("Pending", "files", 12, &["src/a.rs:43:list"]),
                ("LeftTask", "files", 15, &[]),
            ],
        )]});
        let out = back(root, after);
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        for line in ["`wave` em `src/a.rs` linha 3", "`copy` em `src/a.rs` linha 4", "`files` em `src/a.rs` linha 15"] {
            assert!(hint.contains(&format!("{line} é código novo sem uso fora de teste")), "{line}: {hint}");
        }
        assert!(!hint.contains("quem a chamava"), "no field is an orphan: {hint}");
        assert!(!hint.contains("linha 5 ") && !hint.contains("linha 12 "), "the fields that kept their readers stay out: {hint}");
    }

    #[test]
    fn a_field_with_no_reader_in_the_map_before_and_after_is_not_an_orphan_for_the_used_field_of_the_same_name() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // O campo de um tipo tem leitor e o de mesmo nome de outro tipo do
        // arquivo não tem, antes e depois (o mapa não liga a leitura dele): o
        // que a onda mexeu foi o leitor do primeiro, que segue lá.
        let map = |used: &str| {
            json!({"modules": [fields_module(
                "src/a.rs",
                &[
                    ("Markup", "open", 4, &[used]),
                    ("Piece", "open", 8, &[]),
                    ("Level", "fields", 12, &[used]),
                    ("Doc", "fields", 16, &[]),
                    ("WrittenText", "comments", 20, &[used]),
                    ("Walked", "comments", 24, &[]),
                ],
            )]})
        };
        project(root, &[("src/a.rs", "struct Markup {}\n")], &map("src/a.rs:30:code_of"));
        silent(&back(root, map("src/a.rs:31:code_of")));
    }

    #[test]
    fn a_field_of_the_same_type_that_loses_its_last_reader_is_still_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let map = |kept_used: &[&str]| {
            json!({"modules": [fields_module(
                "src/a.rs",
                &[("Kept", "wave", 3, kept_used), ("Keeping", "wave", 5, &["src/a.rs:41:show"])],
            )]})
        };
        project(root, &[("src/a.rs", "struct Kept {}\n")], &map(&["src/a.rs:40:read"]));
        let out = back(root, map(&[]));
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`wave` em `src/a.rs` linha 3 ficou sem uso fora de teste"), "{hint}");
        assert!(!hint.contains("linha 5"), "the field of the other type keeps its reader: {hint}");
    }

    #[test]
    fn a_function_unused_before_the_wave_still_stays_out() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // A onda mexe em `src/lib_sum.rs`, e `never_called` já não tinha uso
        // fora de teste antes dela: mudou de linha, e não foi a onda que a pôs.
        let base = json!({"modules": [module("src/lib_sum.rs", &[("never_called", 5, &["tests/lib_sum_test.rs:3:checks"])])]});
        project_at(root, "src/lib_sum.rs", &[("src/lib_sum.rs", "fn never_called() {}\n")], &base);
        let after = json!({"modules": [module("src/lib_sum.rs", &[("never_called", 9, &["tests/lib_sum_test.rs:3:checks"])])]});
        silent(&back_at(root, "src/lib_sum.rs", after));
    }

    #[test]
    fn a_removed_name_still_declared_elsewhere_gives_no_text_leftover() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let files = [("src/b.rs", "// chama o shared_total da outra pasta\n"), ("src/c.rs", "fn shared_total() {}\n")];
        let base = json!({"modules": [module("src/a.rs", &[("shared_total", 1, &[])]), module("src/c.rs", &[("shared_total", 1, &[])])]});
        project(root, &files, &base);
        let after = json!({"modules": [module("src/a.rs", &[]), module("src/c.rs", &[("shared_total", 1, &[])])]});
        silent(&back(root, after));
    }

    #[test]
    fn the_spec_the_change_log_and_the_history_do_not_count() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let files = [("CHANGELOG.md", "- tira o old_total\n"), ("docs/NEWS", "old_total saiu\n")];
        project(root, &files, &json!({"modules": [module("src/a.rs", &[("old_total", 1, &[])])]}));
        // O nome fica também na spec e numa mensagem de commit.
        let said = write(root, "x", "message", json!({"author": "user", "text": "O old_total sai nesta onda."}));
        assert!(said["id"].is_u64(), "{said}");
        git_at(root, &["commit", "-q", "--allow-empty", "-m", "prepara a saída do old_total"]);
        silent(&back(root, json!({"modules": [module("src/a.rs", &[])]})));
    }

    #[test]
    fn a_registered_route_with_no_caller_is_not_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let routed = |used: &[&str]| {
            let mut routes = module("src/routes.rs", &[("list_orders", 4, used)]);
            routes["routes"] = json!([{"method": "GET", "path": "/orders", "handler": "list_orders", "line": 4, "called_by": []}]);
            json!({"modules": [module("src/a.rs", &[("run_sum", 1, &[])]), routes]})
        };
        project(root, &[("src/routes.rs", "fn list_orders() {}\n")], &routed(&["src/a.rs:2:run_sum"]));
        silent(&back(root, routed(&[])));
    }

    /// O mapa de `src/a.rs`, com o trecho de teste nas linhas 10 a 30: a
    /// função `compute_total`, do programa, na linha 5, e a ajudante
    /// `sample_rows`, do teste, na linha 12, cada uma com quem a usa.
    fn with_test_block(total_used: &[&str], rows_used: &[&str]) -> Value {
        let mut file = module("src/a.rs", &[
            ("run_sum", 1, &[]),
            ("compute_total", 5, total_used),
            ("sample_rows", 12, rows_used),
            ("checks_total", 15, &[]),
        ]);
        file["test_lines"] = json!([[10, 30]]);
        json!({"modules": [file]})
    }

    #[test]
    fn a_helper_inside_the_test_block_that_loses_its_use_is_not_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let base = with_test_block(&["src/a.rs:2:run_sum"], &["src/a.rs:16:checks_total"]);
        project(root, &[("src/a.rs", "fn run_sum() {}\n")], &base);
        silent(&back(root, with_test_block(&["src/a.rs:2:run_sum"], &[])));
    }

    #[test]
    fn a_program_function_beside_the_test_block_that_loses_its_use_is_still_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let base = with_test_block(&["src/a.rs:2:run_sum"], &["src/a.rs:16:checks_total"]);
        project(root, &[("src/a.rs", "fn run_sum() {}\n")], &base);
        let out = back(root, with_test_block(&[], &[]));
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`compute_total` em `src/a.rs` linha 5 ficou sem uso fora de teste"), "{hint}");
        assert!(!hint.contains("sample_rows"), "the test helper is not listed: {hint}");
    }

    #[test]
    fn a_program_function_left_called_only_from_the_test_block_is_an_orphan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let base = with_test_block(&["src/a.rs:2:run_sum", "src/a.rs:16:checks_total"], &[]);
        project(root, &[("src/a.rs", "fn run_sum() {}\n")], &base);
        let out = back(root, with_test_block(&["src/a.rs:16:checks_total"], &[]));
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`compute_total` em `src/a.rs` linha 5 ficou sem uso fora de teste"), "{hint}");
    }

    #[test]
    fn a_program_function_used_only_from_the_test_block_before_the_wave_is_not_listed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let base = with_test_block(&["src/a.rs:16:checks_total"], &[]);
        project(root, &[("src/a.rs", "fn run_sum() {}\n")], &base);
        silent(&back(root, with_test_block(&[], &[])));
    }

    #[test]
    fn a_program_function_still_cited_in_another_file_outside_tests_only_warns() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // A função passada como valor, sem ser chamada ali, é uso que o mapa
        // não liga: ele liga a chamada, não o nome entregue a outra função.
        let go = "use crate::a as calc;\n\nfn go() -> Vec<u32> {\n    [1, 2].into_iter().map(calc::compute_total).collect()\n}\n";
        let files = [("src/a.rs", "fn run_sum() {}\n"), ("src/b.rs", go)];
        project(root, &files, &with_test_block(&["src/a.rs:2:run_sum"], &["src/a.rs:16:checks_total"]));
        let out = back(root, with_test_block(&[], &[]));
        assert_eq!(out["ok"], json!(true), "{out}");
        let warned = out["warnings"].as_array().cloned().unwrap_or_default();
        let hint = warned.iter().find(|w| w["reason"] == json!("round-after-wave-warnings")).map(|w| w["hint"].to_string());
        let hint = hint.unwrap_or_else(|| panic!("the warning: {out}"));
        assert!(hint.contains("`compute_total` em `src/a.rs` linha 5 ficou sem uso fora de teste"), "{hint}");
    }

    #[test]
    fn a_program_function_cited_only_by_tests_or_its_own_lines_still_refuses() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // O nome aparece na linha da declaração, dentro do corpo dela, no
        // trecho de teste do arquivo (linhas 10 a 30) e num arquivo de teste.
        let mut lines = vec![""; 30];
        lines[0] = "fn run_sum() {}";
        lines[4] = "fn compute_total() {";
        lines[5] = "    compute_total();";
        lines[6] = "}";
        lines[9] = "#[cfg(test)]";
        lines[10] = "mod tests {";
        lines[11] = "    fn sample_rows() { super::compute_total(); }";
        lines[29] = "}";
        let text = lines.join("\n") + "\n";
        let files = [("src/a.rs", text.as_str()), ("tests/total_test.rs", "fn checks() { compute_total(); }\n")];
        project(root, &files, &with_test_block(&["src/a.rs:2:run_sum"], &["src/a.rs:16:checks_total"]));
        let out = back(root, with_test_block(&[], &[]));
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`compute_total` em `src/a.rs` linha 5 ficou sem uso fora de teste"), "{hint}");
    }

    /// As três línguas dos testes do membro lido pelo texto: a língua do mapa,
    /// o arquivo que a onda muda, o arquivo de um leitor que ela não muda, o
    /// tipo da declaração e o nome dela.
    const MEMBERS: [(&str, &str, &str, &str, &str); 3] = [
        ("rust", "src/a.rs", "src/reader.rs", "field", "comments"),
        ("typescript", "src/a.ts", "src/reader.ts", "field", "comments"),
        ("csharp", "src/A.cs", "src/Reader.cs", "property", "Comments"),
    ];

    /// O mapa com o membro `name` do tipo `Walked`, declarado na linha 1 de
    /// `file`, lido pelos `used` (`arquivo:linha:quem`).
    fn member_map(language: &str, kind: &str, file: &str, name: &str, used: &[String]) -> Value {
        let member = json!({"kind": kind, "name": name, "line": 1, "end_line": 1, "used_by": used, "owner": ["Walked"]});
        json!({"modules": [{"path": file, "language": language, "declarations": [member]}]})
    }

    #[test]
    fn a_field_read_by_text_after_a_variable_of_an_inferred_type_is_not_an_orphan() {
        // O caso do campo `comments`: lido em `walked.comments` depois de
        // `let walked = Walked::of(root)`, cujo tipo o arquivo não escreve, e
        // que o mapa de depois não liga ao campo.
        for (language, file, _, kind, name) in MEMBERS {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let text = format!(
                "struct Walked {{}}\n\nfn walk() {{\n    let walked = Walked::of(root);\n    let n = walked.{name}.len();\n}}\n"
            );
            let read = [format!("{file}:5:walk")];
            project_at(root, file, &[(file, text.as_str())], &member_map(language, kind, file, name, &read));
            silent(&back_at(root, file, member_map(language, kind, file, name, &[])));
        }
    }

    #[test]
    fn a_field_read_by_text_in_a_file_that_read_it_in_the_base_is_not_an_orphan() {
        // O leitor está num arquivo que a onda não mudou, e o mapa de depois
        // deixou de ligar a leitura dele ao campo.
        for (language, file, reader, kind, name) in MEMBERS {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let text = format!("fn show(walked: &Walked) -> usize {{\n    let w = view(walked);\n    w.{name}.len()\n}}\n");
            let read = [format!("{file}:5:walk"), format!("{reader}:3:show")];
            let files = [(file, "struct Walked {}\n"), (reader, text.as_str())];
            project_at(root, file, &files, &member_map(language, kind, file, name, &read));
            silent(&back_at(root, file, member_map(language, kind, file, name, &[])));
        }
    }

    #[test]
    fn a_field_no_text_reads_any_more_is_still_an_orphan_in_every_language() {
        // O nome só aparece dentro de outro maior (`comments_total`,
        // `xcomments`): não é leitura dele, e ele nem está citado.
        for (language, file, _, kind, name) in MEMBERS {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let text = format!("struct Walked {{}}\n\nfn walk() {{\n    let n = walked.{name}_total + x{name};\n}}\n");
            let read = [format!("{file}:5:walk")];
            project_at(root, file, &[(file, text.as_str())], &member_map(language, kind, file, name, &read));
            let out = back_at(root, file, member_map(language, kind, file, name, &[]));
            assert_eq!(out["reason"], json!("round-after-wave"), "{language}: {out}");
            let hint = out["hint"].as_str().unwrap_or_default();
            assert!(hint.contains(&format!("`{name}` em `{file}` linha 1 ficou sem uso fora de teste")), "{language}: {hint}");
        }
    }

    #[test]
    fn a_name_written_only_in_a_comment_a_range_or_a_test_file_is_not_a_read_of_the_field() {
        // O nome está escrito fora de teste (comentário, faixa `0..nome`), e
        // por isso o aviso não recusa, mas segue saindo: nenhuma dessas
        // linhas lê o campo, e o arquivo de teste não conta.
        for (language, file, _, kind, name) in MEMBERS {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let text =
                format!("struct Walked {{}}\n\n// lê o walked.{name} depois\nfn walk() {{\n    for i in 0..{name} {{}}\n}}\n");
            let tested = format!("fn checks() {{ walked.{name}; }}\n");
            let files = [(file, text.as_str()), ("tests/walked_test.rs", tested.as_str())];
            let read = [format!("{file}:5:walk")];
            project_at(root, file, &files, &member_map(language, kind, file, name, &read));
            let out = back_at(root, file, member_map(language, kind, file, name, &[]));
            assert_eq!(out["ok"], json!(true), "{language}: {out}");
            let warned = out["warnings"].as_array().cloned().unwrap_or_default();
            let hint = warned.iter().find(|w| w["reason"] == json!("round-after-wave-warnings")).map(|w| w["hint"].to_string());
            let hint = hint.unwrap_or_else(|| panic!("{language}: the warning: {out}"));
            assert!(hint.contains(&format!("`{name}` em `{file}` linha 1 ficou sem uso fora de teste")), "{language}: {hint}");
        }
    }

    /// O texto de `src/a.rs` nos testes do código novo: `run_sum` na linha 1,
    /// `compute_total` nas linhas 5 a 7 e o trecho de teste nas linhas 10 a
    /// 30, com `checks_total` (linha 15) que só chama `compute_total` e
    /// `checks_both` (linha 19) que chama também `run_sum`.
    fn new_total_text() -> String {
        let mut lines = vec![""; 30];
        lines[0] = "fn run_sum() {}";
        lines[4] = "fn compute_total() {";
        lines[6] = "}";
        lines[9] = "#[cfg(test)]";
        lines[10] = "mod tests {";
        lines[14] = "    fn checks_total() { assert_eq!(super::compute_total(), 3); }";
        lines[18] = "    fn checks_both() { assert_eq!(super::compute_total(), run_sum()); }";
        lines[29] = "}";
        lines.join("\n") + "\n"
    }

    /// O mapa de `src/a.rs` com `compute_total` já posta pela onda (a base
    /// só tem `run_sum`) e o trecho de teste nas linhas 10 a 30; `used` são
    /// os usos de `compute_total` no mapa.
    fn with_new_total(used: &[&str]) -> Value {
        let mut file = module("src/a.rs", &[
            ("run_sum", 1, &[]),
            ("compute_total", 5, used),
            ("checks_total", 15, &[]),
            ("checks_both", 19, &[]),
        ]);
        file["test_lines"] = json!([[10, 30]]);
        json!({"modules": [file]})
    }

    fn base_with_run_sum_only() -> Value {
        json!({"modules": [module("src/a.rs", &[("run_sum", 1, &[])])]})
    }

    #[test]
    fn a_new_function_only_a_test_calls_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let text = new_total_text();
        project(root, &[("src/a.rs", text.as_str())], &base_with_run_sum_only());
        let head = git_text(root, &["rev-parse", "HEAD"]);
        let out = back(root, with_new_total(&[]));
        assert_eq!(out["ok"], json!(false), "{out}");
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`compute_total` em `src/a.rs` linha 5 é código novo sem uso fora de teste"), "{hint}");
        assert!(!hint.contains("quem a chamava"), "it is new code, not an orphan: {hint}");
        assert!(!hint.contains("`run_sum` em"), "the function that was already there stays out: {hint}");
        assert_eq!(git_text(root, &["rev-parse", "HEAD"]), head, "nothing committed: {out}");
    }

    #[test]
    fn a_test_that_only_calls_a_new_unused_function_is_listed_with_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let text = new_total_text();
        project(root, &[("src/a.rs", text.as_str())], &base_with_run_sum_only());
        let out = back(root, with_new_total(&[]));
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`src/a.rs` linha 15 é um teste que só chama `compute_total`"), "{hint}");
        assert!(!hint.contains("linha 19"), "the test that also calls `run_sum` is not only for it: {hint}");
    }

    #[test]
    fn a_test_file_that_only_calls_a_new_unused_function_is_listed_with_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let files = [("src/a.rs", "fn run_sum() {}\n\n\n\nfn compute_total() {}\n"), ("tests/total_test.rs", "fn checks() { compute_total(); }\n")];
        let tested = module("tests/total_test.rs", &[("checks", 1, &[])]);
        let after = |used: &[&str]| json!({"modules": [module("src/a.rs", &[("run_sum", 1, &[]), ("compute_total", 5, used)]), tested]});
        project(root, &files, &base_with_run_sum_only());
        let out = back(root, after(&["tests/total_test.rs:1:checks"]));
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`compute_total` em `src/a.rs` linha 5 é código novo sem uso fora de teste"), "{hint}");
        assert!(hint.contains("`tests/total_test.rs` linha 1 é um teste que só chama `compute_total`"), "{hint}");
    }

    #[test]
    fn a_new_function_with_a_program_caller_passes() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let text = new_total_text();
        project(root, &[("src/a.rs", text.as_str())], &base_with_run_sum_only());
        silent(&back(root, with_new_total(&["src/a.rs:2:run_sum"])));
    }

    #[test]
    fn a_new_function_in_a_new_file_only_a_test_calls_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // O arquivo da onda não está no mapa da base: o que ele declara é novo.
        let files = [("src/a.rs", "fn run_sum() {}\n"), ("src/lib_sum.rs", "\n\n\n\nfn compute_total() {}\n")];
        project_at(root, "src/lib_sum.rs", &files, &base_with_run_sum_only());
        let after = json!({"modules": [
            module("src/a.rs", &[("run_sum", 1, &[])]),
            module("src/lib_sum.rs", &[("compute_total", 5, &["tests/lib_sum_test.rs:3:checks"])]),
        ]});
        let out = back_at(root, "src/lib_sum.rs", after);
        assert_eq!(out["reason"], json!("round-after-wave"), "{out}");
        let hint = out["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("`compute_total` em `src/lib_sum.rs` linha 5 é código novo sem uso fora de teste"), "{hint}");
    }

    #[test]
    fn a_new_function_still_cited_outside_tests_only_warns() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // A função entregue como valor a outra, sem ser chamada ali, é uso que o
        // mapa não liga.
        let go = "use crate::a as calc;\n\nfn go() -> Vec<u32> {\n    [1, 2].into_iter().map(calc::compute_total).collect()\n}\n";
        let text = new_total_text();
        project(root, &[("src/a.rs", text.as_str()), ("src/b.rs", go)], &base_with_run_sum_only());
        let out = back(root, with_new_total(&[]));
        assert_eq!(out["ok"], json!(true), "{out}");
        let warned = out["warnings"].as_array().cloned().unwrap_or_default();
        let hint = warned.iter().find(|w| w["reason"] == json!("round-after-wave-warnings")).map(|w| w["hint"].to_string());
        let hint = hint.unwrap_or_else(|| panic!("the warning: {out}"));
        assert!(hint.contains("`compute_total` em `src/a.rs` linha 5 é código novo sem uso fora de teste"), "{hint}");
        assert!(!hint.contains("é um teste que só chama"), "the name may be a use, so no test is called useless: {hint}");
    }

    #[test]
    fn a_new_helper_or_test_gated_to_tests_is_not_unused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // O scan só guarda como trecho de teste o módulo com `#[cfg(test)]`
        // exato: o apoio e o teste com o atributo em cima chegam como código.
        let text = "fn run_sum() {}\n#[cfg(test)]\nfn seed_value() {}\n/// Um teste.\n#[test]\nfn sums_it() {}\n";
        let after = json!({"modules": [module("src/a.rs", &[("run_sum", 1, &[]), ("seed_value", 3, &[]), ("sums_it", 6, &[])])]});
        project(root, &[("src/a.rs", text)], &base_with_run_sum_only());
        silent(&back(root, after));
    }

    #[test]
    fn a_new_entry_point_or_route_handler_is_not_unused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Cada declaração nova do arquivo não tem uso nenhum e é ponto de
        // entrada: a função principal, o método de rota, o tipo que o tem, o
        // método que cumpre um contrato e a implementação de um traço de fora.
        let decl = |kind: &str, name: &str, line: u64| json!({"kind": kind, "name": name, "line": line, "end_line": line + 1});
        let mut controller = decl("class", "OrdersController", 3);
        controller["members"] = json!(["src/routes.rs:4:list_orders"]);
        let mut handler = decl("method", "list_orders", 4);
        handler["owner"] = json!(["OrdersController"]);
        let mut fulfils = decl("method", "handle", 7);
        fulfils["implements"] = json!(["src/ports.rs:2:handle"]);
        let mut shows = decl("method", "fmt", 10);
        shows["contract"] = json!(["Display"]);
        let mut routes = json!({"path": "src/routes.rs", "language": "rust", "declarations": [
            decl("function", "main", 1), controller, handler, fulfils, shows,
        ]});
        routes["routes"] = json!([{"method": "GET", "path": "/orders", "handler": "list_orders", "line": 4, "called_by": []}]);
        project_at(root, "src/routes.rs", &[("src/routes.rs", "fn main() {}\n")], &base_with_run_sum_only());
        silent(&back_at(root, "src/routes.rs", json!({"modules": [routes]})));
    }

    #[test]
    fn a_new_field_read_by_text_is_not_unused() {
        // O campo novo `comments`, lido em `walked.comments` depois de uma
        // variável de tipo inferido, que o mapa de depois não liga a ele.
        for (language, file, _, kind, name) in MEMBERS {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let text = format!(
                "struct Walked {{}}\n\nfn walk() {{\n    let walked = Walked::of(root);\n    let n = walked.{name}.len();\n}}\n"
            );
            let walked = json!({"kind": "struct", "name": "Walked", "line": 1, "end_line": 1});
            let before = json!({"modules": [{"path": file, "language": language, "declarations": [walked.clone()]}]});
            project_at(root, file, &[(file, text.as_str())], &before);
            let member = json!({"kind": kind, "name": name, "line": 2, "end_line": 2, "owner": ["Walked"]});
            let after = json!({"modules": [{"path": file, "language": language, "declarations": [walked, member]}]});
            silent(&back_at(root, file, after));
        }
    }

    #[test]
    fn a_new_field_no_text_reads_is_unused() {
        for (language, file, _, kind, name) in MEMBERS {
            let dir = tempdir().unwrap();
            let root = dir.path();
            let text = format!("struct Walked {{}}\n\nfn walk() {{\n    let n = walked.{name}_total + x{name};\n}}\n");
            let walked = json!({"kind": "struct", "name": "Walked", "line": 1, "end_line": 1});
            let before = json!({"modules": [{"path": file, "language": language, "declarations": [walked.clone()]}]});
            project_at(root, file, &[(file, text.as_str())], &before);
            let member = json!({"kind": kind, "name": name, "line": 2, "end_line": 2, "owner": ["Walked"]});
            let after = json!({"modules": [{"path": file, "language": language, "declarations": [walked, member]}]});
            let out = back_at(root, file, after);
            assert_eq!(out["reason"], json!("round-after-wave"), "{language}: {out}");
            let hint = out["hint"].as_str().unwrap_or_default();
            assert!(hint.contains(&format!("`{name}` em `{file}` linha 2 é código novo sem uso fora de teste")), "{language}: {hint}");
        }
    }
}
