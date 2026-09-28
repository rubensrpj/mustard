//! `pattern` — o padrão do projeto aprendido do código: o papel de cada
//! arquivo e as direções de importação que o código já segue, pela força.
//!
//! Três passos sobre o grafo de importações do mapa:
//!
//! 1. o papel de cada arquivo, sem lista de nomes: o sufixo do nome — a
//!    última palavra do nome sem a extensão, quando ele tem duas ou mais —
//!    que se repete em [`SUFFIX_FILES`] arquivos do mapa ou mais
//!    (`pedido.service.ts`, `pedido_service.go` e `PedidoService.cs` dão
//!    `service`); o nome de uma palavra só que se repete sob
//!    [`FOLDER_PARENTS`] pastas diferentes (`views.py`, `handler.go`);
//!    senão, a pasta mais próxima cujo nome se repete sob [`FOLDER_PARENTS`]
//!    pais diferentes; senão, a unidade de topo — o projeto do censo, ou a
//!    pasta logo abaixo da pasta que guarda o código dele inteiro (`src/`).
//!    O arquivo de entrada da pasta, pela língua do arquivo (o `index` que
//!    responde pela pasta), nunca ganha papel pelo nome nem conta para achar
//!    um: o nome dele é o mesmo em toda pasta por regra da língua, não por
//!    escolha do time, e não diz camada. Ele fica com o papel da pasta ou da
//!    unidade, como qualquer arquivo cujo nome não diz papel;
//! 2. o arquivo que concentra a contramão vira papel próprio: das
//!    importações de um lado que vai contra a maioria do par, ou de um lado
//!    de um par sem direção, [`AGAINST_IMPORTS`] ou mais, com
//!    [`CONCENTRATION_PERCENT`]% ou mais chegando a ele. O motivo vai junto;
//! 3. a direção de cada par de papéis com [`PAIR_IMPORTS`] importações ou
//!    mais: [`STRONG_PERCENT`]% ou mais num sentido é regra forte; de
//!    [`INFO_PERCENT`]% até ela, informação; abaixo, sem direção.
//!
//! Só contam as importações resolvidas a arquivo do projeto (`deps`), e
//! nenhuma que parte de arquivo de teste ou chega a ele. Com mais de um
//! projeto no censo, o papel leva o nome do projeto na frente. Nenhum nome
//! de framework, de língua ou de estilo: o papel sai de como os nomes se
//! repetem.
//!
//! Função pura, sobre o mapa lido na hora: cada atualização do mapa já a
//! refaz, e nada fica guardado à parte.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::domain::ast::{is_entry_file, is_test_path};
use crate::domain::project_map::{MapModule, MapProject, ProjectMap};

/// Em quantos arquivos do projeto o sufixo do nome se repete para virar
/// papel.
const SUFFIX_FILES: usize = 5;

/// Sob quantos pais diferentes o nome de uma pasta, ou o nome de uma palavra
/// só de um arquivo, se repete, no projeto, para virar papel.
const FOLDER_PARENTS: usize = 3;

/// Quantas importações, somados os dois sentidos, um par de papéis precisa
/// para ter direção.
const PAIR_IMPORTS: usize = 10;

/// A parte das importações do par num sentido, em por cento, que faz a regra
/// forte. Regras aprendidas três meses antes, em três projetos, e conferidas
/// nas importações nascidas depois: a de 95% ou mais não teve nenhuma
/// importação real contra.
const STRONG_PERCENT: usize = 95;

/// A parte num sentido, em por cento, que faz a informação; abaixo dela, o
/// par fica sem direção. Na mesma conferência, uma direção de 82% teve 25
/// importações contra, todas o mesmo hábito do time: por isso ela só informa.
const INFO_PERCENT: usize = 80;

/// Quantas importações um lado contrário precisa para que o arquivo que as
/// concentra vire papel próprio.
const AGAINST_IMPORTS: usize = 10;

/// A parte das importações do lado contrário, em por cento, que chega ao
/// arquivo que vira papel próprio.
const CONCENTRATION_PERCENT: usize = 75;

/// O padrão aprendido do mapa: o papel de cada arquivo que conta, os
/// arquivos que viraram papel próprio, as regras fortes e as informações.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pattern {
    /// O papel de cada arquivo fora de teste, pelo caminho. O arquivo que
    /// virou papel próprio tem o próprio caminho como papel.
    pub roles: BTreeMap<String, String>,
    /// Os arquivos que viraram papel próprio, com o motivo, na ordem em que
    /// a conta os achou.
    pub own_roles: Vec<OwnRole>,
    /// Os pares com [`STRONG_PERCENT`]% ou mais das importações num sentido,
    /// do par com mais importações para o com menos.
    pub strong: Vec<Direction>,
    /// Os pares com [`INFO_PERCENT`]% ou mais num sentido, abaixo da regra
    /// forte, na mesma ordem.
    pub info: Vec<Direction>,
}

/// Um arquivo que virou papel próprio e o motivo: das `of` importações do
/// papel `from` para o papel `to`, que iam contra a maioria do par ou
/// ficavam num par sem direção, `received` chegavam a ele.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnRole {
    pub path: String,
    pub from: String,
    pub to: String,
    pub received: usize,
    pub of: usize,
}

/// A direção de um par de papéis: o papel `from` importa o papel `to` em
/// `along` importações, e `to` importa `from` em `against`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Direction {
    pub from: String,
    pub to: String,
    pub along: usize,
    pub against: usize,
}

impl Pattern {
    /// A regra forte que a importação do papel `from` para o papel `to`
    /// contraria: a direção em que `to` importa `from`. `None` quando a
    /// importação segue as regras, ou quando os dois papéis são o mesmo.
    #[must_use]
    pub fn strong_against(&self, from: &str, to: &str) -> Option<&Direction> {
        against(&self.strong, from, to)
    }

    /// A informação (a direção fraca) que a importação do papel `from` para o
    /// papel `to` contraria, como [`Pattern::strong_against`].
    #[must_use]
    pub fn info_against(&self, from: &str, to: &str) -> Option<&Direction> {
        against(&self.info, from, to)
    }

    /// As importações do arquivo `module` seguem as regras fortes: nenhuma
    /// vai de encontro a uma delas, pelos papéis que este padrão dá.
    #[must_use]
    pub fn follows(&self, module: &MapModule) -> bool {
        let Some(own) = self.roles.get(&module.path) else { return true };
        module.deps.iter().filter_map(|dep| self.roles.get(dep)).all(|role| self.strong_against(own, role).is_none())
    }
}

/// A direção de `list` em que `to` importa `from`, com papéis diferentes.
fn against<'d>(list: &'d [Direction], from: &str, to: &str) -> Option<&'d Direction> {
    (from != to).then(|| list.iter().find(|d| d.from == to && d.to == from)).flatten()
}

/// A língua do arquivo `module`: a que o mapa gravou, ou, no mapa lido só
/// em parte, sem ela, a que a extensão do caminho diz. É por ela que o
/// arquivo de entrada da pasta se reconhece, e a leitura em parte dá os
/// mesmos papéis que o mapa inteiro.
fn language_of(module: &MapModule) -> &str {
    if module.language.is_empty() {
        crate::domain::source_lang::language_of_path(&module.path).unwrap_or_default()
    } else {
        &module.language
    }
}

/// O padrão do projeto do mapa `map`: os três passos do começo do módulo.
#[must_use]
pub fn learn(map: &ProjectMap) -> Pattern {
    let files: Vec<&MapModule> = map.modules.iter().filter(|m| !is_test_path(&m.path)).collect();
    let mut roles = roles_of(&files, &map.projects);
    let known: BTreeSet<&str> = files.iter().map(|m| m.path.as_str()).collect();
    let edges: BTreeSet<(&str, &str)> = files
        .iter()
        .flat_map(|m| m.deps.iter().map(move |dep| (m.path.as_str(), dep.as_str())))
        .filter(|&(from, to)| from != to && known.contains(to))
        .collect();
    let edges: Vec<(&str, &str)> = edges.into_iter().collect();
    let mut own_roles = Vec::new();
    loop {
        let found = concentrated(&edges, &roles);
        if found.is_empty() {
            break;
        }
        for own in found {
            roles.insert(own.path.clone(), own.path.clone());
            own_roles.push(own);
        }
    }
    let (strong, info) = directions(&pairs(&edges, &roles));
    Pattern { roles, own_roles, strong, info }
}

/// As importações entre papéis diferentes, por par em ordem — o papel de
/// quem importa e o de quem é importado —, com o arquivo importado de cada
/// uma.
type Pairs<'r, 'e> = BTreeMap<(&'r str, &'r str), Vec<&'e str>>;

fn pairs<'r, 'e>(edges: &[(&'e str, &'e str)], roles: &'r BTreeMap<String, String>) -> Pairs<'r, 'e> {
    let mut out: Pairs<'r, 'e> = BTreeMap::new();
    for &(from, to) in edges {
        let (Some(a), Some(b)) = (roles.get(from), roles.get(to)) else { continue };
        if a != b {
            out.entry((a.as_str(), b.as_str())).or_default().push(to);
        }
    }
    out
}

/// `part` é pelo menos `percent`% de `total`.
fn at_least(part: usize, total: usize, percent: usize) -> bool {
    part * 100 >= percent * total
}

/// O par com estes dois lados tem direção: importações bastantes e a parte
/// da informação num dos sentidos.
fn has_direction(one: usize, other: usize) -> bool {
    let total = one + other;
    total >= PAIR_IMPORTS && at_least(one.max(other), total, INFO_PERCENT)
}

/// Os arquivos que concentram a contramão, com os papéis de agora: em cada
/// lado que vai contra a maioria do par, ou em qualquer lado de um par sem
/// direção, com [`AGAINST_IMPORTS`] importações ou mais, o arquivo que
/// recebe [`CONCENTRATION_PERCENT`]% delas ou mais. Um arquivo por vez,
/// pelo primeiro lado em que aparece.
fn concentrated(edges: &[(&str, &str)], roles: &BTreeMap<String, String>) -> Vec<OwnRole> {
    let pairs = pairs(edges, roles);
    let mut found: BTreeMap<&str, OwnRole> = BTreeMap::new();
    for (&(from, to), targets) in &pairs {
        let back = pairs.get(&(to, from)).map_or(0, Vec::len);
        let against = !has_direction(targets.len(), back) || targets.len() < back;
        if !against || targets.len() < AGAINST_IMPORTS {
            continue;
        }
        let mut received: BTreeMap<&str, usize> = BTreeMap::new();
        for target in targets {
            *received.entry(target).or_default() += 1;
        }
        let Some((&file, &count)) = received.iter().max_by_key(|&(path, count)| (*count, Reverse(*path))) else {
            continue;
        };
        if at_least(count, targets.len(), CONCENTRATION_PERCENT) && roles.get(file).is_none_or(|role| role != file) {
            found.entry(file).or_insert_with(|| OwnRole {
                path: file.to_string(),
                from: from.to_string(),
                to: to.to_string(),
                received: count,
                of: targets.len(),
            });
        }
    }
    found.into_values().collect()
}

/// As regras fortes e as informações dos pares, do par com mais importações
/// para o com menos.
fn directions(pairs: &Pairs<'_, '_>) -> (Vec<Direction>, Vec<Direction>) {
    let (mut strong, mut info) = (Vec::new(), Vec::new());
    for (&(from, to), targets) in pairs {
        let along = targets.len();
        let against = pairs.get(&(to, from)).map_or(0, Vec::len);
        if along <= against || !has_direction(along, against) {
            continue;
        }
        let direction = Direction { from: from.to_string(), to: to.to_string(), along, against };
        if at_least(along, along + against, STRONG_PERCENT) {
            strong.push(direction);
        } else {
            info.push(direction);
        }
    }
    for list in [&mut strong, &mut info] {
        list.sort_by_key(|d| Reverse(d.along + d.against));
    }
    (strong, info)
}

/// Os arquivos de um projeto do censo: o nome dele, quantas pastas os
/// arquivos dele que moram em pasta têm em comum no começo, e cada arquivo
/// pelo caminho inteiro e pelas partes do caminho a partir da pasta do
/// projeto.
struct Group<'a> {
    name: &'a str,
    common: usize,
    files: Vec<(&'a str, Vec<&'a str>)>,
}

/// As pastas do caminho `segments` abaixo da pasta comum do projeto: é nelas
/// que o nome diz o papel ou a unidade. O arquivo solto na pasta do projeto
/// não tem nenhuma.
fn inner<'s>(segments: &'s [&'s str], common: usize) -> &'s [&'s str] {
    segments[..segments.len() - 1].get(common..).unwrap_or(&[])
}

/// O papel de cada arquivo (o primeiro passo do começo do módulo). O sufixo,
/// o nome de uma palavra só e a pasta se contam no mapa inteiro; a unidade de topo é o projeto do
/// arquivo e, quando o código do projeto mora numa pasta só (`src/`), a pasta
/// logo abaixo dela; o arquivo solto na pasta do projeto, como o de
/// compilação, não conta para achar essa pasta. Com um projeto só, o nome
/// dele não separa nada, e a unidade é sempre a pasta em que os arquivos se
/// separam. O nome do arquivo de entrada da pasta não dá papel: nem o fim
/// dele, nem a palavra só.
fn roles_of(files: &[&MapModule], projects: &[MapProject]) -> BTreeMap<String, String> {
    let groups = groups_of(files, projects);
    let several = groups.len() > 1;
    let entries: BTreeSet<&str> =
        files.iter().filter(|m| is_entry_file(&m.path, language_of(m))).map(|m| m.path.as_str()).collect();
    let suffixes = repeated_suffixes(files);
    let single_words = repeated_single_words(files);
    let folders = repeated_folders(&groups);
    let mut roles = BTreeMap::new();
    for group in &groups {
        let named = several && !group.name.is_empty();
        for (path, segments) in &group.files {
            let inner = inner(segments, group.common);
            let folder = || inner.iter().rev().map(|s| s.to_lowercase()).find(|s| folders.contains(s));
            let word = || single_word_of(path).filter(|w| single_words.contains(w));
            let suffix = || suffix_of(path).filter(|s| suffixes.contains(s));
            let by_name = if entries.contains(path) { None } else { suffix().or_else(word) };
            let role = by_name.or_else(folder);
            let label = match (role, inner.first()) {
                (Some(role), _) if named => format!("{}:{role}", group.name),
                (Some(role), _) => role,
                (None, Some(unit)) if named && group.common > 0 => format!("{}:{unit}", group.name),
                (None, _) if named => group.name.to_string(),
                (None, Some(_)) => segments[..=group.common].join("/"),
                (None, None) if segments.len() > 1 => segments[..segments.len() - 1].join("/"),
                (None, None) => ".".to_string(),
            };
            roles.insert((*path).to_string(), label);
        }
    }
    roles
}

/// Os arquivos por projeto do censo: cada um é do projeto mais fundo cuja
/// pasta o contém; o que nenhum contém fica num grupo sem nome.
fn groups_of<'a>(files: &[&'a MapModule], projects: &'a [MapProject]) -> Vec<Group<'a>> {
    let mut groups: BTreeMap<&str, Group<'a>> = BTreeMap::new();
    for module in files {
        let path = module.path.as_str();
        let project = projects
            .iter()
            .filter(|p| p.dir.is_empty() || path.strip_prefix(p.dir.as_str()).is_some_and(|rest| rest.starts_with('/')))
            .max_by_key(|p| p.dir.len());
        let (dir, name) = project.map_or(("", ""), |p| (p.dir.as_str(), p.name.as_str()));
        let relative = path.strip_prefix(dir).map_or(path, |rest| rest.trim_start_matches('/'));
        let group = groups.entry(dir).or_insert_with(|| Group { name, common: 0, files: Vec::new() });
        group.files.push((path, relative.split('/').collect()));
    }
    let mut groups: Vec<Group<'a>> = groups.into_values().collect();
    for group in &mut groups {
        group.common = common_depth(&group.files);
    }
    groups
}

/// Quantas pastas os arquivos que moram em pasta têm em comum no começo.
fn common_depth(files: &[(&str, Vec<&str>)]) -> usize {
    let mut folders = files.iter().map(|(_, segments)| &segments[..segments.len() - 1]).filter(|f| !f.is_empty());
    let Some(first) = folders.next() else { return 0 };
    folders.fold(first.len(), |common, other| first[..common].iter().zip(other).take_while(|(a, b)| a == b).count())
}

/// As palavras do nome do arquivo sem a extensão, em minúsculas. O nome se
/// separa em `.`, `_` e `-` e na troca de minúscula para maiúscula; a sigla
/// em maiúsculas fica uma palavra só até a última maiúscula antes de uma
/// minúscula: `IPedidoService` dá `i`, `pedido`, `service`; `HTTPClient` dá
/// `http`, `client`; `pedido_service` dá `pedido`, `service`.
fn words_of(path: &str) -> Vec<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let mut words = Vec::new();
    for piece in stem.split(['.', '_', '-']) {
        let letters: Vec<char> = piece.chars().collect();
        let mut word = String::new();
        for (at, &letter) in letters.iter().enumerate() {
            let before = at.checked_sub(1).map(|b| letters[b]);
            let after = letters.get(at + 1);
            let from_lower = before.is_some_and(char::is_lowercase) && letter.is_uppercase();
            let ends_acronym = before.is_some_and(char::is_uppercase)
                && letter.is_uppercase()
                && after.is_some_and(|a| a.is_lowercase());
            if (from_lower || ends_acronym) && !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            word.extend(letter.to_lowercase());
        }
        if !word.is_empty() {
            words.push(word);
        }
    }
    words
}

/// O sufixo do nome do arquivo: a última palavra de [`words_of`], quando o
/// nome tem duas ou mais, com ponto entre elas ou sem (`service` em
/// `pedido.service.ts`, `pedido_service.go` e `PedidoService.cs`); `None` no
/// nome de uma palavra só.
fn suffix_of(path: &str) -> Option<String> {
    let mut words = words_of(path);
    if words.len() < 2 {
        return None;
    }
    words.pop()
}

/// O nome do arquivo, quando ele é uma palavra só (`views` em `views.py`);
/// `None` no nome de duas palavras ou mais.
fn single_word_of(path: &str) -> Option<String> {
    let mut words = words_of(path);
    if words.len() != 1 {
        return None;
    }
    words.pop()
}

/// Os nomes de uma palavra só que se repetem sob [`FOLDER_PARENTS`] pastas
/// diferentes ou mais no mapa: o mesmo nome em pastas diferentes diz o papel
/// do arquivo, como `views.py` em cada módulo ou `handler.go` em cada pacote.
/// O arquivo de entrada da pasta, pela língua dele, não conta: o nome dele se
/// repete em toda pasta por regra da língua.
fn repeated_single_words(files: &[&MapModule]) -> BTreeSet<String> {
    let mut folders: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for module in files.iter().filter(|m| !is_entry_file(&m.path, &m.language)) {
        if let Some(word) = single_word_of(&module.path) {
            let folder = module.path.rsplit_once('/').map_or("", |(folder, _)| folder);
            folders.entry(word).or_default().insert(folder);
        }
    }
    folders.into_iter().filter(|(_, above)| above.len() >= FOLDER_PARENTS).map(|(word, _)| word).collect()
}

/// Os sufixos do nome que se repetem em [`SUFFIX_FILES`] arquivos do mapa ou
/// mais.
fn repeated_suffixes(files: &[&MapModule]) -> BTreeSet<String> {
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for suffix in files.iter().filter_map(|m| suffix_of(&m.path)) {
        *count.entry(suffix).or_default() += 1;
    }
    count.into_iter().filter(|&(_, n)| n >= SUFFIX_FILES).map(|(suffix, _)| suffix).collect()
}

/// Os nomes de pasta, em minúsculas, que se repetem sob [`FOLDER_PARENTS`]
/// pais diferentes ou mais no mapa. Só contam as pastas abaixo da pasta comum
/// de cada projeto: a pasta que guarda o código inteiro do projeto não diz
/// papel.
fn repeated_folders(groups: &[Group<'_>]) -> BTreeSet<String> {
    let mut parents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for group in groups {
        for (path, segments) in &group.files {
            let whole: Vec<&str> = path.split('/').collect();
            let above = whole.len() - segments.len();
            for at in group.common..segments.len() - 1 {
                parents.entry(segments[at].to_lowercase()).or_default().insert(whole[..above + at].join("/"));
            }
        }
    }
    parents.into_iter().filter(|(_, above)| above.len() >= FOLDER_PARENTS).map(|(name, _)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um arquivo do mapa, com as importações resolvidas.
    fn module(path: &str, deps: &[String]) -> MapModule {
        MapModule { path: path.to_string(), deps: deps.to_vec(), ..MapModule::default() }
    }

    /// Os arquivos `src/<papel>/<n>.<papel>.ts`, de 0 a `count - 1`.
    fn named(role: &str, count: usize) -> Vec<String> {
        (0..count).map(|n| format!("src/{role}/{role}{n}.{role}.ts")).collect()
    }

    /// Um mapa com `a_files` arquivos de papel `a` e `b_files` de papel `b`,
    /// `along` importações de `a` para `b` e `against` de `b` para `a`, cada
    /// importação de um par de arquivos diferente.
    fn two_roles(a_files: usize, b_files: usize, along: usize, against: usize) -> ProjectMap {
        let (a, b) = (named("controller", a_files), named("service", b_files));
        let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let pairs = a.iter().flat_map(|from| b.iter().map(move |to| (from, to)));
        for (from, to) in pairs.clone().take(along) {
            deps.entry(from.clone()).or_default().push(to.clone());
        }
        for (to, from) in pairs.skip(along).take(against) {
            deps.entry(from.clone()).or_default().push(to.clone());
        }
        let modules =
            a.iter().chain(&b).map(|path| module(path, deps.get(path).map_or(&[][..], Vec::as_slice))).collect();
        ProjectMap { modules, ..ProjectMap::default() }
    }

    fn direction(from: &str, to: &str, along: usize, against: usize) -> Direction {
        Direction { from: from.to_string(), to: to.to_string(), along, against }
    }

    #[test]
    fn a_pair_with_96_percent_in_one_sense_is_a_strong_rule() {
        let pattern = learn(&two_roles(5, 5, 24, 1));
        assert_eq!(pattern.strong, [direction("controller", "service", 24, 1)]);
        assert!(pattern.info.is_empty(), "{pattern:?}");
    }

    #[test]
    fn a_pair_with_85_percent_is_only_information() {
        let pattern = learn(&two_roles(5, 5, 17, 3));
        assert!(pattern.strong.is_empty(), "{pattern:?}");
        assert_eq!(pattern.info, [direction("controller", "service", 17, 3)]);
    }

    #[test]
    fn the_same_map_with_the_pair_falling_to_90_percent_loses_the_rule() {
        assert_eq!(learn(&two_roles(5, 5, 19, 1)).strong.len(), 1, "95% is a rule");
        let pattern = learn(&two_roles(5, 5, 18, 2));
        assert!(pattern.strong.is_empty(), "{pattern:?}");
        assert_eq!(pattern.info, [direction("controller", "service", 18, 2)]);
    }

    #[test]
    fn a_pair_with_9_imports_gives_nothing() {
        let pattern = learn(&two_roles(5, 5, 9, 0));
        assert!(pattern.strong.is_empty() && pattern.info.is_empty(), "{pattern:?}");
        assert!(!learn(&two_roles(5, 5, 10, 0)).strong.is_empty(), "10 imports are enough");
    }

    #[test]
    fn an_import_from_a_test_file_does_not_count() {
        let mut map = two_roles(5, 5, 9, 0);
        // Fora da regra de teste, o arquivo teria o papel `controller` pelo
        // sufixo e faria a décima importação do par.
        let test_file = "src/controller/test_pedido.controller.ts";
        map.modules.push(module(test_file, &named("service", 5)[..1]));
        let pattern = learn(&map);
        assert!(pattern.strong.is_empty() && pattern.info.is_empty(), "{pattern:?}");
        assert!(!pattern.roles.contains_key(test_file));
    }

    /// Um mapa em que 62 repositórios importam o mesmo arquivo de serviço, 4
    /// importam outro serviço, e os serviços importam os repositórios 190
    /// vezes.
    fn one_file_takes_the_wrong_way() -> ProjectMap {
        let services = named("service", 20);
        let repositories = named("repository", 62);
        let client = "src/service/client.service.ts".to_string();
        let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (from, to) in services.iter().flat_map(|s| repositories.iter().map(move |r| (s, r))).take(190) {
            deps.entry(from.clone()).or_default().push(to.clone());
        }
        for repository in &repositories {
            deps.entry(repository.clone()).or_default().push(client.clone());
        }
        for repository in &repositories[..4] {
            deps.entry(repository.clone()).or_default().push(services[0].clone());
        }
        let modules = services
            .iter()
            .chain(&repositories)
            .chain(std::iter::once(&client))
            .map(|path| module(path, deps.get(path).map_or(&[][..], Vec::as_slice)))
            .collect();
        ProjectMap { modules, ..ProjectMap::default() }
    }

    #[test]
    fn the_file_that_receives_62_of_66_wrong_way_imports_is_a_role_of_its_own_and_the_pair_gets_a_sense() {
        let pattern = learn(&one_file_takes_the_wrong_way());
        let client = "src/service/client.service.ts";
        assert_eq!(
            pattern.own_roles,
            [OwnRole { path: client.into(), from: "repository".into(), to: "service".into(), received: 62, of: 66 }]
        );
        assert_eq!(pattern.roles[client], client);
        assert_eq!(
            pattern.strong,
            [direction("service", "repository", 190, 4), direction("repository", client, 62, 0)]
        );
    }

    #[test]
    fn with_several_projects_the_role_carries_the_project_and_code_in_one_folder_splits_below_it() {
        let project = |name: &str, dir: &str| MapProject { name: name.into(), dir: dir.into(), ..MapProject::default() };
        let expected = [
            ("apps/rt/build.rs", "rt"),
            ("apps/rt/src/main.rs", "rt"),
            ("apps/rt/src/commands/map.rs", "rt:commands"),
            ("Api/Program.cs", "Api"),
            ("Api/Endpoints/Pedido.cs", "Api"),
            ("Api/Services/Pedido.cs", "Api:services"),
            ("Data/Contexts/Contexto.cs", "Data"),
            ("Data/Services/Banco.cs", "Data:services"),
            ("Core/Entities/Modelo.cs", "Core"),
            ("Core/Services/Regra.cs", "Core:services"),
        ];
        let map = ProjectMap {
            modules: expected.iter().map(|(path, _)| module(path, &[])).collect(),
            projects: vec![
                project("rt", "apps/rt"),
                project("Api", "Api"),
                project("Data", "Data"),
                project("Core", "Core"),
            ],
            ..ProjectMap::default()
        };
        let roles = learn(&map).roles;
        for (path, role) in expected {
            assert_eq!(roles[path], role, "{path}");
        }
    }

    #[test]
    fn a_suffix_repeated_in_4_files_is_not_a_role() {
        let mut map = two_roles(5, 5, 10, 0);
        for n in 0..4 {
            map.modules.push(module(&format!("src/extra{n}/pedido{n}.handler.ts"), &[]));
        }
        let pattern = learn(&map);
        assert!(!pattern.roles.values().any(|role| role == "handler"), "{:?}", pattern.roles);
        map.modules.push(module("src/extra4/pedido4.handler.ts", &[]));
        assert!(learn(&map).roles.values().any(|role| role == "handler"), "5 files make the role");
    }

    /// O papel de cada arquivo de `paths`, num mapa só com eles e sem
    /// importação nenhuma.
    fn roles_of_paths(paths: &[String]) -> BTreeMap<String, String> {
        let map = ProjectMap { modules: paths.iter().map(|path| module(path, &[])).collect(), ..ProjectMap::default() };
        learn(&map).roles
    }

    /// O papel de cada arquivo de `files`, dado pelo caminho e pela língua,
    /// num mapa só com eles e sem importação nenhuma.
    fn roles_of_files(files: &[(String, &str)]) -> BTreeMap<String, String> {
        let modules = files
            .iter()
            .map(|(path, language)| MapModule {
                path: path.clone(),
                language: (*language).to_string(),
                ..MapModule::default()
            })
            .collect();
        learn(&ProjectMap { modules, ..ProjectMap::default() }).roles
    }

    /// Os arquivos `<pasta>/<pasta de módulo>/<nome>` de cada pasta de
    /// módulo de `modules`, na língua `language`.
    fn in_folders(folder: &str, modules: &[&str], name: &str, language: &'static str) -> Vec<(String, &'static str)> {
        modules.iter().map(|m| (format!("{folder}/{m}/{name}"), language)).collect()
    }

    const FOLDERS: [&str; 5] = ["pedidos", "clientes", "produtos", "estoques", "faturas"];

    #[test]
    fn five_folder_entry_files_take_the_role_of_their_unit_and_never_their_name() {
        let files = in_folders("src", &FOLDERS, "index.ts", "typescript");
        let roles = roles_of_files(&files);
        assert!(!roles.values().any(|role| role == "index"), "{roles:?}");
        assert_eq!(roles["src/pedidos/index.ts"], "src/pedidos");
    }

    #[test]
    fn module_and_package_entry_files_give_no_role_by_their_name() {
        let files = [
            in_folders("src", &FOLDERS, "mod.rs", "rust"),
            in_folders("app", &FOLDERS[..3], "__init__.py", "python"),
        ]
        .concat();
        let roles = roles_of_files(&files);
        assert!(!roles.values().any(|role| role == "mod" || role == "init"), "{roles:?}");
    }

    #[test]
    fn a_single_word_name_that_is_no_entry_file_of_its_language_keeps_its_role() {
        let views = in_folders("app", &FOLDERS[..3], "views.py", "python");
        assert_eq!(not_in_role(&roles_of_files(&views), &paths_of(&views), "views"), []);
        // A língua sem arquivo de entrada dá papel ao `main` repetido.
        let mains = in_folders("cmd", &FOLDERS[..3], "main.go", "go");
        assert_eq!(not_in_role(&roles_of_files(&mains), &paths_of(&mains), "main"), []);
    }

    /// O mapa lido só em parte, sem a língua de cada arquivo, dá os mesmos
    /// papéis que o mapa inteiro: o arquivo de entrada se reconhece pela
    /// extensão do caminho.
    #[test]
    fn a_map_read_without_languages_gives_the_same_roles() {
        let files = [
            in_folders("src", &FOLDERS, "index.ts", "typescript"),
            in_folders("lib", &FOLDERS, "mod.rs", "rust"),
            in_folders("app", &FOLDERS[..3], "views.py", "python"),
        ]
        .concat();
        let without: Vec<(String, &str)> = files.iter().map(|(path, _)| (path.clone(), "")).collect();
        assert_eq!(roles_of_files(&without), roles_of_files(&files));
    }

    /// A importação contra a regra forte é achada pelos dois papéis, só no
    /// sentido contrário ao da regra; a que vai contra a informação, só como
    /// informação; e o arquivo que importa contra a regra forte não a segue.
    #[test]
    fn the_import_against_a_rule_is_found_by_its_roles() {
        let strong = learn(&two_roles(5, 5, 24, 1));
        assert_eq!(strong.strong_against("service", "controller"), Some(&direction("controller", "service", 24, 1)));
        assert_eq!(strong.strong_against("controller", "service"), None);
        assert_eq!(strong.info_against("service", "controller"), None);
        let weak = learn(&two_roles(5, 5, 17, 3));
        assert_eq!(weak.strong_against("service", "controller"), None);
        assert_eq!(weak.info_against("service", "controller"), Some(&direction("controller", "service", 17, 3)));
        let map = two_roles(5, 5, 24, 1);
        let against = map.modules.iter().find(|m| m.path.ends_with(".service.ts") && !m.deps.is_empty()).unwrap();
        assert!(!strong.follows(against), "{against:?}");
        assert!(map.modules.iter().filter(|m| m.path.ends_with(".controller.ts")).all(|m| strong.follows(m)));
    }

    /// Os caminhos de `files`, sem a língua.
    fn paths_of(files: &[(String, &str)]) -> Vec<String> {
        files.iter().map(|(path, _)| path.clone()).collect()
    }

    /// Os caminhos de `paths` cujo papel não é `role`, com o papel que têm.
    fn not_in_role<'p>(roles: &BTreeMap<String, String>, paths: &'p [String], role: &str) -> Vec<(&'p str, String)> {
        paths.iter().filter(|path| roles[*path] != role).map(|path| (path.as_str(), roles[path].clone())).collect()
    }

    const MODULES: [&str; 5] = ["Pedido", "Cliente", "Produto", "Estoque", "Fatura"];

    #[test]
    fn five_controllers_named_in_camel_case_in_module_folders_have_the_controller_role() {
        let paths: Vec<String> = MODULES.iter().map(|m| format!("{m}s/{m}Controller.cs")).collect();
        let roles = roles_of_paths(&paths);
        assert_eq!(not_in_role(&roles, &paths, "controller"), []);
    }

    #[test]
    fn five_services_named_with_an_underscore_and_a_handler_in_three_packages_have_their_roles() {
        let services: Vec<String> =
            MODULES.iter().map(|m| m.to_lowercase()).map(|m| format!("internal/{m}s/{m}_service.go")).collect();
        let handlers: Vec<String> =
            MODULES[..3].iter().map(|m| format!("internal/{}s/handler.go", m.to_lowercase())).collect();
        let roles = roles_of_paths(&[services.clone(), handlers.clone()].concat());
        let missed = (not_in_role(&roles, &services, "service"), not_in_role(&roles, &handlers, "handler"));
        assert_eq!(missed, (vec![], vec![]));
    }

    #[test]
    fn views_in_three_modules_have_the_views_role() {
        let paths: Vec<String> = ["pedidos", "clientes", "produtos"].iter().map(|m| format!("{m}/views.py")).collect();
        let roles = roles_of_paths(&paths);
        assert_eq!(not_in_role(&roles, &paths, "views"), []);
    }

    #[test]
    fn five_dotted_services_keep_the_service_role() {
        let paths: Vec<String> = MODULES.iter().map(|m| format!("src/{}/pedido.service.ts", m.to_lowercase())).collect();
        let roles = roles_of_paths(&paths);
        assert_eq!(not_in_role(&roles, &paths, "service"), []);
    }

    #[test]
    fn a_single_word_name_in_two_folders_only_is_not_a_role() {
        let paths: Vec<String> = ["pedidos", "clientes"].iter().map(|m| format!("{m}/views.py")).collect();
        let roles = roles_of_paths(&paths);
        assert!(!roles.values().any(|role| role == "views"), "{roles:?}");
    }

    #[test]
    fn a_suffix_without_a_dot_in_four_files_only_is_not_a_role() {
        let mut paths: Vec<String> =
            MODULES[..4].iter().map(|m| m.to_lowercase()).map(|m| format!("internal/{m}s/{m}_service.go")).collect();
        assert!(!roles_of_paths(&paths).values().any(|role| role == "service"), "4 files are not enough");
        paths.push("internal/faturas/fatura_service.go".to_string());
        assert_eq!(not_in_role(&roles_of_paths(&paths), &paths, "service"), [], "5 files make the role");
    }

    #[test]
    fn the_words_of_a_name_split_at_separators_and_case_and_keep_an_acronym_whole() {
        assert_eq!(words_of("src/IPedidoService.cs"), ["i", "pedido", "service"]);
        assert_eq!(words_of("HTTPClient.ts"), ["http", "client"]);
        assert_eq!(words_of("pkg/pedido_service.go"), ["pedido", "service"]);
        assert_eq!(words_of("src/pedido.service.ts"), ["pedido", "service"]);
        assert_eq!(words_of("app/views.py"), ["views"]);
        assert_eq!(suffix_of("Pedidos/PedidoController.cs").as_deref(), Some("controller"));
        assert_eq!(suffix_of("app/views.py"), None);
        assert_eq!(single_word_of("app/views.py").as_deref(), Some("views"));
    }
}
