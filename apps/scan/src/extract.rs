//! Layer 2 — Syntactic extraction, language-agnostic.
//!
//! One generic tree-sitter engine drives every language. A language is defined
//! entirely by DATA: a row in `languages.toml` (name, extensions, grammar) and a
//! set of `.scm` query files under `queries/<dir>/`. This module never names a
//! language, an extension, or a grammar node — it only understands a small,
//! generic capture vocabulary that every query speaks. The whole list, with
//! what each capture means, lives in `queries/README.md`, the one place it is
//! written.
//!
//! Several things come off the tree itself, with no grammar node name: the
//! documentation comment above a declaration (the `extra` nodes the grammar
//! attaches right above it), the line where the declaration starts (the first
//! decoration that comment's path passes over, when there is one), its
//! signature (its own text up to the body or up to the value), the call sites
//! of the file (a named leaf that reads as an identifier and is followed by an
//! opening parenthesis), and the names it cites without calling (the same
//! leaf, not followed by a parenthesis, whatever letter it starts with: which
//! of them name something of the project is decided by `graph`, with the whole
//! project in hand). A grammar that marks none of it simply yields nothing, as
//! with every other generic rule here.
//!
//! The per-language seam the old design called for is preserved: there is one
//! [`Analyzer`] instance per language, but all are the same generic type, each
//! parameterized by a compiled query. Precise AST facts go in; the same generic
//! `Extracted`/`Decl` come out, so the graph and the map never learn any syntax.
//!
//! `build.rs` embeds the registry and the query files into `OUT_DIR`; we include
//! the generated table here. Nothing language-specific lives in this file.

use crate::model::{CallSite, Decl, Route, RouteLinks, Text, RECEIVER, TEXT_ERROR, TEXT_LOG, TEXT_PLAIN};
use crate::routes::{self, RouteRule};
use mustard_core::domain::project_map::outer_declarations;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor};

#[derive(Default)]
pub(crate) struct Extracted {
    pub imports: Vec<String>,
    /// The imports the language puts in sight of more files than the one that
    /// writes them (`@import.global`).
    pub global_imports: Vec<String>,
    /// Os imports escritos dentro de um trecho de teste (`@test_block`): ficam
    /// fora de `imports` e de `global_imports`.
    pub test_imports: Vec<String>,
    /// As linhas, da primeira à última, de cada trecho de teste do arquivo.
    pub test_lines: Vec<(usize, usize)>,
    /// As linhas, da primeira à última, de cada módulo com corpo escrito
    /// dentro do arquivo (`@inner_module`).
    pub module_lines: Vec<(usize, usize)>,
    /// As linhas de cada import que o arquivo escreve ao menos uma vez dentro
    /// de um desses módulos, todas elas.
    pub import_lines: BTreeMap<String, Vec<usize>>,
    /// Cada caminho do projeto escrito antes do nome numa chamada
    /// (`@call.path` que virou import), com as chamadas escritas por ele, fora
    /// do trecho de teste.
    pub call_paths: BTreeMap<String, Vec<CallSite>>,
    /// Cada caminho de duas partes ou mais escrito antes do nome numa chamada
    /// que não virou import (`std::fs` em `std::fs::read()`), com as chamadas
    /// escritas por ele, fora do trecho de teste.
    pub other_call_paths: BTreeMap<String, Vec<CallSite>>,
    /// De cada import, os nomes que ele traz ao arquivo (`@imported`),
    /// ordenados e sem repetição.
    pub brought: BTreeMap<String, Vec<String>>,
    /// De cada repasse escrito fora dos módulos de dentro do arquivo, os nomes
    /// que ele oferece, cada um com o nome que tem no arquivo de origem; `*`
    /// quando oferece todos.
    pub reexports: BTreeMap<String, BTreeMap<String, String>>,
    pub namespaces: Vec<String>,
    pub declarations: Vec<Decl>,
    pub calls: Vec<CallSite>,
    pub cites: Vec<CallSite>,
    /// Os nomes que abrem a cadeia escrita antes de uma chamada e que o
    /// arquivo não liga ([`crate::model::Module::unbound_heads`]).
    pub unbound_heads: Vec<String>,
    /// Os textos fixos do arquivo (`@text`), em ordem de linha.
    pub texts: Vec<Text>,
    /// As rotas do servidor registradas no arquivo, fora do trecho de teste.
    pub routes: Vec<Route>,
    /// Os prefixos que o arquivo escreve para rotas de outros arquivos.
    pub route_links: RouteLinks,
    /// Os comentários do começo do arquivo, antes do primeiro código, numa
    /// linha.
    pub file_doc: String,
    /// Os outros comentários do arquivo que caem fora das linhas de toda
    /// declaração, numa linha: os de dentro já estão no `body_comment` dela.
    pub file_comment: String,
    /// Quantos bytes do começo do `body_comment` da primeira declaração de
    /// fora são comentários do começo do arquivo.
    pub file_doc_in_body: usize,
}

/// O que a extração de um arquivo lê além das declarações, dos imports, das
/// chamadas e das citações: o que o mapa não guarda do arquivo não se lê.
#[derive(Clone, Copy)]
pub(crate) struct Keep {
    /// Os comentários e os nomes escritos no código, do arquivo e de cada
    /// declaração: o arquivo escrito por máquina não os guarda.
    pub written_text: bool,
    /// Os textos fixos e as rotas: só o código do projeto os guarda, e não o
    /// arquivo de teste nem o escrito por máquina.
    pub texts_and_routes: bool,
}

/// One language as produced by `build.rs` from `languages.toml` + its `.scm`
/// files. The grammar is already resolved to a tree-sitter [`Language`].
/// (Extensions live in the separate `LANG_EXTENSIONS` table used for detection.)
pub struct RawLang {
    pub name: &'static str,
    pub query: &'static str,
    pub language: Language,
    /// The markup tags a documentation comment of the language is written
    /// with (`doc_tags` in languages.toml): the engine drops them and keeps
    /// the text. Empty when the language writes its comments in plain prose.
    pub doc_tags: &'static [&'static str],
}

// Brings `raw_langs()` and `LANG_EXTENSIONS` into scope — generated from the
// external language registry; see build.rs. This is the only place the grammar
// symbols are referenced, and it lives in OUT_DIR, not in src/.
include!(concat!(env!("OUT_DIR"), "/langs_generated.rs"));

/// Detect a file's language purely from data (the registry's extension table).
/// No `match` on extensions, no hardcoded mapping — adding a language to
/// `languages.toml` extends detection automatically.
pub fn detect_language(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    LANG_EXTENSIONS
        .iter()
        .find(|(_, exts)| exts.iter().any(|e| *e == ext))
        .map(|(name, _)| (*name).to_string())
}

/// Root-alias segments a language uses to alias the package root in qualified
/// import paths — pure registry data (`root_aliases` in languages.toml). A
/// language that declares none gets an empty slice, which disables the graph's
/// root-alias resolution branch for its modules.
pub fn root_aliases(lang: &str) -> &'static [&'static str] {
    LANG_ROOT_ALIASES
        .iter()
        .find(|(name, _)| *name == lang)
        .map(|(_, aliases)| *aliases)
        .unwrap_or(&[])
}

/// How a language's declared namespace is seen by its other files — pure
/// registry data (`namespace_scope` in languages.toml): `"folder"`, `"nested"`,
/// or empty when the language declares none.
pub fn namespace_scope(lang: &str) -> &'static str {
    LANG_NAMESPACE_SCOPE.iter().find(|(name, _)| *name == lang).map_or("", |(_, scope)| *scope)
}

/// The file extensions of a language, as the registry writes them.
pub fn extensions(lang: &str) -> &'static [&'static str] {
    LANG_EXTENSIONS.iter().find(|(name, _)| *name == lang).map_or(&[], |(_, exts)| *exts)
}

/// As extensões que o import da língua escreve no lugar da do próprio arquivo
/// — dado do registro (`import_extensions` em languages.toml). Vazio quando a
/// língua não declara nenhuma.
pub fn import_extensions(lang: &str) -> &'static [&'static str] {
    LANG_IMPORT_EXTENSIONS.iter().find(|(name, _)| *name == lang).map_or(&[], |(_, exts)| *exts)
}

/// O import relativo que a língua escreve com um separador no lugar da barra
/// (`relative_import` em languages.toml).
#[derive(Clone, Copy, Debug)]
pub struct RelativeImport {
    /// O texto entre as partes do caminho; repetido no começo, torna o import
    /// relativo à pasta de quem importa.
    pub separator: &'static str,
    /// O nome, sem extensão, do arquivo que responde pela pasta; vazio quando
    /// a língua não tem.
    pub package_file: &'static str,
}

impl RelativeImport {
    /// Quantas vezes o separador abre o import, e o resto dele. Zero quando o
    /// import não começa pelo separador: não é relativo nesta forma.
    pub fn leading<'a>(&self, imp: &'a str) -> (usize, &'a str) {
        let mut rest = imp;
        let mut count = 0;
        while let Some(after) = rest.strip_prefix(self.separator) {
            rest = after;
            count += 1;
        }
        (count, rest)
    }
}

/// O import relativo por separador da língua — dado do registro. `None`
/// quando a língua não o declara: o import dela nunca é lido assim.
pub fn relative_import(lang: &str) -> Option<RelativeImport> {
    LANG_RELATIVE_IMPORT
        .iter()
        .find(|(name, separator, _)| *name == lang && !separator.is_empty())
        .map(|(_, separator, package_file)| RelativeImport { separator, package_file })
}

/// Os textos que juntam as partes de um nome qualificado na língua
/// (`qualified_separators` em languages.toml), na ordem em que o registro os
/// escreve. `None` quando a língua não os declara: quem lê usa a regra de
/// sempre.
pub fn qualified_separators(lang: &str) -> Option<&'static [&'static str]> {
    LANG_QUALIFIED_SEPARATORS
        .iter()
        .find(|(name, separators)| *name == lang && !separators.is_empty())
        .map(|(_, separators)| *separators)
}

/// O nome que, no começo de um caminho qualificado, sobe um módulo
/// (`parent_alias` em languages.toml). `None` quando a língua não o declara.
pub fn parent_alias(lang: &str) -> Option<&'static str> {
    Some(text_field(LANG_PARENT_ALIAS, lang)).filter(|alias| !alias.is_empty())
}

/// Os separadores que ligam um nome ao qualificador escrito antes dele: os da
/// língua ou, quando ela não os declara, `::` e `.`.
fn qualifier_separators(lang: &str) -> &'static [&'static str] {
    qualified_separators(lang).unwrap_or(&["::", "."])
}

/// Os separadores que ligam o método ao valor escrito antes dele sem juntar
/// caminho (`member_separators` em languages.toml). Vazio sem o campo.
fn member_separators(lang: &str) -> &'static [&'static str] {
    list_field(LANG_MEMBER_SEPARATORS, lang)
}

/// A língua liga o método ao valor por um separador próprio: o de nome
/// qualificado, nela, só junta caminho, e o nome antes dele é módulo ou tipo.
pub fn has_member_separators(lang: &str) -> bool {
    !member_separators(lang).is_empty()
}

/// Os nomes que, antes do método chamado, são o próprio objeto ou o próprio
/// tipo (`self_receivers` em languages.toml). Vazio sem o campo.
pub fn self_receivers(lang: &str) -> &'static [&'static str] {
    list_field(LANG_SELF_RECEIVERS, lang)
}

/// A língua chama um membro do próprio objeto pelo nome sozinho
/// (`implicit_self` em languages.toml). `false` sem o campo.
pub fn implicit_self(lang: &str) -> bool {
    LANG_IMPLICIT_SELF.iter().any(|&(name, on)| name == lang && on)
}

/// O nome que, trazido por um import, traz o último nome escrito antes da
/// lista que o contém (`import_self` em languages.toml). `None` sem o campo.
fn import_self(lang: &str) -> Option<&'static str> {
    Some(text_field(LANG_IMPORT_SELF, lang)).filter(|name| !name.is_empty())
}

/// Os nomes que todo arquivo da língua vê sem import (`prelude` em
/// languages.toml). Vazio sem o campo.
pub fn prelude(lang: &str) -> &'static [&'static str] {
    list_field(LANG_PRELUDE, lang)
}

/// As chamadas que escrevem no log (`log_calls` em languages.toml). Vazio
/// sem o campo.
fn log_calls(lang: &str) -> &'static [&'static str] {
    list_field(LANG_LOG_CALLS, lang)
}

/// O que lança ou devolve erro (`error_forms` em languages.toml). Vazio sem
/// o campo.
fn error_forms(lang: &str) -> &'static [&'static str] {
    list_field(LANG_ERROR_FORMS, lang)
}

/// Os nomes do próprio objeto visto pelo tipo de cima (`parent_receivers` em
/// languages.toml). Vazio sem o campo.
pub fn parent_receivers(lang: &str) -> &'static [&'static str] {
    list_field(LANG_PARENT_RECEIVERS, lang)
}

/// Os arquivos raiz de um pacote, sem a extensão e a partir da pasta do
/// manifesto (`package_entry` em languages.toml). Vazio sem o campo.
pub fn package_entry(lang: &str) -> &'static [&'static str] {
    list_field(LANG_PACKAGE_ENTRY, lang)
}

/// A família da língua: as línguas que leem as mesmas consultas (`dir` em
/// languages.toml) são a mesma, escrita em arquivos de extensões diferentes.
/// Vazio para uma língua que o registro não tem.
pub fn family(lang: &str) -> &'static str {
    text_field(LANG_FAMILY, lang)
}

/// O valor de um campo de lista do registro para a língua; vazio sem o campo.
fn list_field(table: &'static [(&'static str, &'static [&'static str])], lang: &str) -> &'static [&'static str] {
    table.iter().find(|(name, _)| *name == lang).map_or(&[], |(_, values)| *values)
}

/// O valor de um campo de texto do registro para a língua; vazio sem o campo.
fn text_field(table: &'static [(&'static str, &'static str)], lang: &str) -> &'static str {
    table.iter().find(|(name, _)| *name == lang).map_or("", |(_, value)| *value)
}

/// O nome do arquivo de configuração dos apelidos de pasta da língua
/// (`alias_config` em languages.toml); vazio quando a língua não tem.
pub fn alias_config(lang: &str) -> &'static str {
    text_field(LANG_ALIAS_CONFIG, lang)
}

/// A chave, em caminho com pontos, da pasta base dos imports não relativos
/// (`alias_base` em languages.toml).
pub fn alias_base(lang: &str) -> &'static str {
    text_field(LANG_ALIAS_BASE, lang)
}

/// A chave, em caminho com pontos, do objeto de apelidos de pasta
/// (`alias_paths` em languages.toml).
pub fn alias_paths(lang: &str) -> &'static str {
    text_field(LANG_ALIAS_PATHS, lang)
}

/// A chave, em caminho com pontos, da herança entre configurações
/// (`alias_extends` em languages.toml).
pub fn alias_extends(lang: &str) -> &'static str {
    text_field(LANG_ALIAS_EXTENDS, lang)
}

/// As línguas que declaram arquivo de configuração de apelidos de pasta.
pub fn alias_languages() -> impl Iterator<Item = &'static str> {
    LANG_ALIAS_CONFIG.iter().filter(|(_, file)| !file.is_empty()).map(|(name, _)| *name)
}

/// Todos os nomes de arquivo de configuração de apelidos que o registro
/// declara, sem repetição.
pub fn alias_config_names() -> BTreeSet<&'static str> {
    alias_languages().map(alias_config).collect()
}

/// Build one [`Analyzer`] per language declared in the registry. A language
/// whose grammar/queries fail to compile is skipped with a warning rather than
/// aborting the whole run.
pub fn registry() -> HashMap<String, Analyzer> {
    let mut m = HashMap::new();
    for raw in raw_langs() {
        if let Some(a) = Analyzer::new(&raw) {
            m.insert(a.name.clone(), a);
        }
    }
    m
}

/// What a capture name means to the engine. Computed once per compiled query so
/// the hot path is an index lookup, not a string compare.
enum CapKind {
    Import,
    /// An import that is in sight of every file of the language under the
    /// same project, not only of the file that writes it.
    ImportGlobal,
    /// A name an import brings into the file (`limite` in
    /// `import { limite } from`): it says what the file brought, and like the
    /// import itself it is never a use.
    Imported,
    /// O caminho de um repasse (`export * from './x'`, `pub use a::B`): é
    /// import do arquivo, e os nomes trazidos no mesmo comando são os que o
    /// arquivo oferece a quem o importa, tirados do arquivo que o caminho
    /// nomeia; sem nome nenhum, oferece todos.
    Reexport,
    /// O nome que o arquivo de origem dá ao nome que o repasse do mesmo
    /// pattern oferece com outro (`X` em `export { X as Y } from`).
    ReexportOriginal,
    Namespace,
    /// O caminho escrito antes do nome numa chamada qualificada (`crate::a`
    /// em `crate::a::f()`): vira import do arquivo só quando começa por um
    /// dos `root_aliases` da língua ou pelo `parent_alias` dela.
    CallPath,
    Name,
    Supertype,
    /// O tipo dono escrito fora da declaração do mesmo pattern (o de um bloco
    /// `impl`, o receptor de um método): vem depois dos donos que a contêm no
    /// arquivo.
    Owner,
    /// O contrato que a declaração do mesmo pattern cumpre por onde foi
    /// escrita (o traço de `impl Traço for Tipo`).
    Contract,
    /// An attribute or decorator adorning a declaration: never code of it.
    Decoration,
    /// The body of a declaration that the grammar keeps beside it rather than
    /// inside it: the declaration ends where its body ends.
    Body,
    /// The value a declaration is given: the header ends where it starts.
    Value,
    /// The documentation the language writes inside the declaration rather
    /// than above it; the comment above, when there is one, is worth more.
    Doc,
    /// Um trecho de teste escrito dentro do arquivo: o que se importa nele é
    /// do teste, e o que se chama ou se cita nele não é uso do código.
    TestBlock,
    /// Um módulo com corpo escrito dentro do arquivo: o caminho escrito nele
    /// que começa pelo `parent_alias` da língua sai dele antes de subir pasta.
    InnerModule,
    /// Um nome que o corpo de uma função liga (a variável, o parâmetro, o nome
    /// novo de uma desestruturação): escrito sozinho depois, na mesma
    /// declaração, ele é esse valor, e não a declaração do projeto de mesmo
    /// nome.
    Local,
    /// Um literal de texto escrito no código: vira texto fixo do arquivo
    /// quando tem cara de texto e não cai num import, num trecho de teste
    /// nem numa documentação. `plain` é o texto escrito sem aspas (o texto
    /// solto de uma tela): ele se guarda como está escrito, sem o corte das
    /// aspas.
    Text { plain: bool },
    Def(String),
    Ignore,
}

fn classify(cap: &str) -> CapKind {
    match cap {
        "import" => CapKind::Import,
        "import.global" => CapKind::ImportGlobal,
        "imported" => CapKind::Imported,
        "reexport" => CapKind::Reexport,
        "reexport.original" => CapKind::ReexportOriginal,
        "namespace" => CapKind::Namespace,
        "call.path" => CapKind::CallPath,
        "name" => CapKind::Name,
        "supertype" => CapKind::Supertype,
        "owner" => CapKind::Owner,
        "owner.contract" => CapKind::Contract,
        "decoration" => CapKind::Decoration,
        "body" => CapKind::Body,
        "value" => CapKind::Value,
        "doc" => CapKind::Doc,
        "test_block" => CapKind::TestBlock,
        "inner_module" => CapKind::InnerModule,
        "local" => CapKind::Local,
        "text" => CapKind::Text { plain: false },
        "text.plain" => CapKind::Text { plain: true },
        other => match other.strip_prefix("definition.") {
            Some(kind) => CapKind::Def(kind.to_string()),
            None => CapKind::Ignore,
        },
    }
}

pub(crate) struct Analyzer {
    name: String,
    language: Language,
    query: Query,
    /// `cap_kinds[i]` is the role of capture index `i` in `query`.
    cap_kinds: Vec<CapKind>,
    doc_tags: &'static [&'static str],
    /// As regras de rota dos frameworks escritos na língua.
    routes: Vec<RouteRule>,
}

impl Analyzer {
    fn new(raw: &RawLang) -> Option<Analyzer> {
        let language = raw.language.clone();
        // Compile patterns individually and keep the ones that hold against this
        // grammar version. A single drifted node name then costs one pattern, not
        // the whole language — and never a panic.
        let good = compile_good_patterns(&language, raw.query, raw.name);
        if good.is_empty() {
            eprintln!("grain: no usable query patterns for '{}' — skipping", raw.name);
            return None;
        }
        let combined = good.join("\n");
        let query = match Query::new(&language, &combined) {
            Ok(q) => q,
            Err(e) => {
                eprintln!("grain: query for '{}' failed to compile: {e}", raw.name);
                return None;
            }
        };
        let cap_kinds = query.capture_names().iter().map(|n| classify(n)).collect();
        let routes = routes::rules_for(raw.name, &language);
        Some(Analyzer { name: raw.name.to_string(), language, query, cap_kinds, doc_tags: raw.doc_tags, routes })
    }

    /// As regras de rota da língua que algum arquivo ligou até aqui, e que
    /// por isso já compilaram a consulta, pelo nome.
    pub fn compiled_routes(&self) -> impl Iterator<Item = String> + '_ {
        self.routes.iter().filter(|rule| rule.was_compiled()).map(RouteRule::name)
    }

    /// Alguma regra de rota da língua liga pela dependência do manifesto.
    pub(crate) fn routes_follow_manifests(&self) -> bool {
        self.routes.iter().any(RouteRule::follows_manifest)
    }

    /// Alguma regra de rota da língua liga no arquivo que importa `imports`
    /// com o que o projeto diz em `more`, e não só com o que ele diz em
    /// `less`.
    pub(crate) fn routes_turned_on_by(&self, imports: &[String], less: &routes::Project, more: &routes::Project) -> bool {
        routes::turned_on_by(&self.routes, imports, less, more)
    }

    /// O que o arquivo `src` diz, lido só no que `keep` pede além das
    /// declarações, dos imports, das chamadas e das citações. As rotas saem
    /// das regras que o arquivo liga pelos imports dele ou pelo que o
    /// `project` diz dele.
    pub fn extract(&self, src: &str, keep: Keep, project: &routes::Project) -> Extracted {
        let mut out = Extracted::default();
        let mut parser = Parser::new();
        if parser.set_language(&self.language).is_err() {
            return out;
        }
        let tree = match parser.parse(src, None) {
            Some(t) => t,
            None => return out,
        };
        let bytes = src.as_bytes();
        let root = tree.root_node();
        let mut cursor = QueryCursor::new();

        // Declarations keyed by node start byte (so they emerge in document
        // order) and then by where the name is written and the name itself:
        // one statement may declare several names (`export const a = 1, b =
        // 2;`), and each of them is a declaration of its own. Supertypes keyed
        // by the cleaned declaration name so a base captured in a detached
        // node attaches to the right decl.
        // The comment and the header are read only after every match, once the
        // decorations of the whole file are known: a decoration may be matched
        // after the declaration it adorns.
        let mut decls: BTreeMap<(usize, usize, String), Header> = BTreeMap::new();
        let mut decorations: Spans = BTreeSet::new();
        // What an import or a namespace capture covers: the names written
        // there are the path of the import, not a use of what they name.
        let mut import_spans: Spans = BTreeSet::new();
        let mut supers_by_name: HashMap<String, BTreeSet<String>> = HashMap::new();
        // Os trechos de teste do arquivo, e cada import com o byte e a linha
        // em que foi escrito: só depois de todos os matches se sabe qual cai
        // num trecho de teste ou num módulo do arquivo.
        let mut test_blocks: Spans = BTreeSet::new();
        let mut written: Vec<Written> = Vec::new();
        // Cada nome que um import traz, com o nó em que foi escrito: só depois
        // de todos os matches se sabe de que import ele é.
        let mut imported_at: Vec<(Node, String)> = Vec::new();
        // O nome de origem de cada nome que um repasse oferece com outro, pelo
        // byte em que o nome oferecido foi escrito.
        let mut original_of: HashMap<usize, String> = HashMap::new();
        // Cada nome que o corpo de uma função liga, com a linha e o byte em
        // que foi escrito.
        let mut locals: Vec<(usize, usize, String)> = Vec::new();
        // Os literais de texto do arquivo, cada um dizendo se foi escrito sem
        // aspas, e o que a documentação escrita dentro das declarações cobre:
        // o literal que a contém não é texto fixo.
        let mut literals: Vec<(Node, bool)> = Vec::new();
        // Cada caminho de chamada que não virou import, com o byte em que foi
        // escrito e a chamada que vem depois dele.
        let mut other_paths: Vec<(String, usize, CallSite)> = Vec::new();
        let mut doc_spans: Spans = BTreeSet::new();
        let self_name = import_self(&self.name);

        // O import relativo por separador da língua, quando ela o declara.
        let relative = relative_import(&self.name);
        // O começo que faz do caminho de uma chamada qualificada um lugar do
        // próprio projeto, e o que separa as partes dele.
        let aliases = root_aliases(&self.name);
        let parent = parent_alias(&self.name);
        let separators = qualifier_separators(&self.name);

        // Os comentários do arquivo: o separador escrito num deles não liga
        // um nome ao que vem antes.
        let comments = comment_spans(root);
        let mut matches = cursor.matches(&self.query, root, bytes);
        while let Some(m) = matches.next() {
            let mut def: Option<(Node, &str)> = None;
            let mut name_text: Option<String> = None;
            let mut name_byte = usize::MAX;
            let mut here_supers: Vec<String> = Vec::new();
            // O dono e o contrato escritos fora da declaração deste match.
            let mut here_owner: Vec<String> = Vec::new();
            let mut here_contract: Vec<String> = Vec::new();
            let mut body_end: Option<usize> = None;
            let mut value_start: Option<usize> = None;
            let mut name_kind: &'static str = "";
            let mut doc_inside: Option<(usize, String)> = None;
            // Os imports deste match, com o byte e a linha em que cada um foi
            // escrito, e os nomes que o mesmo pattern diz que eles trazem.
            let mut here_imports: Vec<Written> = Vec::new();
            let mut brought: Vec<String> = Vec::new();
            // O nome oferecido e o de origem, quando o pattern escreve os dois.
            let mut offered_at: Option<usize> = None;
            let mut original: Option<String> = None;

            for cap in m.captures {
                let node = cap.node;
                match &self.cap_kinds[cap.index as usize] {
                    CapKind::Import | CapKind::ImportGlobal | CapKind::Reexport => {
                        import_spans.insert((node.start_byte(), node.end_byte()));
                        if let Ok(t) = node.utf8_text(bytes) {
                            let c = clean_import(t);
                            if !c.is_empty() {
                                let kind = &self.cap_kinds[cap.index as usize];
                                let global = matches!(kind, CapKind::ImportGlobal);
                                let reexport = matches!(kind, CapKind::Reexport);
                                here_imports.push(Written { reexport, ..Written::at(c, global, node) });
                            }
                        }
                    }
                    CapKind::ReexportOriginal => {
                        import_spans.insert((node.start_byte(), node.end_byte()));
                        original = node.utf8_text(bytes).ok().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
                    }
                    CapKind::Imported => {
                        import_spans.insert((node.start_byte(), node.end_byte()));
                        if let Ok(t) = node.utf8_text(bytes) {
                            let t = t.trim();
                            // O nome que traz o último nome antes da lista
                            // (`self` em `use std::fs::{self}`) traz esse nome.
                            let name = match self_name {
                                Some(own) if t == own => name_before_list(node, bytes),
                                _ => Some(t.to_string()).filter(|t| !t.is_empty()),
                            };
                            if let Some(name) = name {
                                imported_at.push((node, name.clone()));
                                offered_at = Some(node.start_byte());
                                brought.push(name);
                            }
                        }
                    }
                    CapKind::Local => {
                        if let Ok(t) = node.utf8_text(bytes)
                            && is_identifier(t)
                        {
                            locals.push((node.start_position().row + 1, node.start_byte(), t.to_string()));
                        }
                    }
                    CapKind::CallPath => {
                        // Só o caminho que começa no próprio projeto é import;
                        // qualquer outro (`Vec::new()`, `std::fs::read()`)
                        // segue como uso, como antes. Os argumentos de tipo
                        // não são parte do lugar que o caminho nomeia. O nome
                        // chamado é o nó nomeado logo depois do caminho, lido
                        // como a chamada dele é lida.
                        if let Ok(t) = node.utf8_text(bytes) {
                            let path = without_type_arguments(&t.split_whitespace().collect::<String>(), separators);
                            let first = first_segment(&path, separators);
                            let call = || node.next_named_sibling().and_then(|n| called_site(n, bytes, &comments, &self.name));
                            if aliases.contains(&first) || parent == Some(first) {
                                import_spans.insert((node.start_byte(), node.end_byte()));
                                here_imports.push(Written { call: call(), ..Written::at(path, false, node) });
                            } else if first.len() < path.len()
                                && let Some(call) = call()
                            {
                                // O de duas partes ou mais fica guardado com a
                                // chamada: o grafo diz se a raiz dele é de
                                // fora do projeto. O de uma parte só
                                // (`Vec::new()`) segue só como qualificador.
                                other_paths.push((path, node.start_byte(), call));
                            }
                        }
                    }
                    CapKind::Namespace => {
                        import_spans.insert((node.start_byte(), node.end_byte()));
                        if let Ok(t) = node.utf8_text(bytes) {
                            let t = t.trim();
                            if !t.is_empty() {
                                out.namespaces.push(t.to_string());
                            }
                        }
                    }
                    CapKind::Name => {
                        if let Ok(t) = node.utf8_text(bytes) {
                            name_text = Some(t.to_string());
                            name_byte = node.start_byte();
                            name_kind = node.kind();
                        }
                    }
                    CapKind::Decoration => {
                        decorations.insert((node.start_byte(), node.end_byte()));
                    }
                    CapKind::Body => {
                        body_end = Some(node.end_position().row + 1);
                    }
                    CapKind::Value => {
                        value_start = Some(node.start_byte());
                    }
                    CapKind::Doc => {
                        doc_spans.insert((node.start_byte(), node.end_byte()));
                        if let Ok(t) = node.utf8_text(bytes) {
                            doc_inside = Some((node.start_byte(), t.to_string()));
                        }
                    }
                    CapKind::Supertype => {
                        if let Ok(t) = node.utf8_text(bytes)
                            && let Some(n) = simple_type_name(t) {
                                here_supers.push(n);
                            }
                    }
                    CapKind::Owner | CapKind::Contract => {
                        if let Ok(t) = node.utf8_text(bytes)
                            && let Some(n) = simple_type_name(t) {
                                let into = if matches!(self.cap_kinds[cap.index as usize], CapKind::Owner) {
                                    &mut here_owner
                                } else {
                                    &mut here_contract
                                };
                                into.push(n);
                            }
                    }
                    CapKind::TestBlock => {
                        test_blocks.insert((node.start_byte(), node.end_byte()));
                        out.test_lines.push((node.start_position().row + 1, node.end_position().row + 1));
                    }
                    CapKind::InnerModule => {
                        out.module_lines.push((node.start_position().row + 1, node.end_position().row + 1));
                    }
                    CapKind::Text { plain } => literals.push((node, *plain)),
                    CapKind::Def(kind) => {
                        def = Some((node, kind.as_str()));
                    }
                    CapKind::Ignore => {}
                }
            }

            // O import relativo feito só do separador nomeia uma pasta, e não
            // um arquivo: cada nome que o mesmo pattern diz que ele traz é um
            // arquivo dessa pasta, e o import vira o separador seguido do
            // nome. Qualquer outro import fica como foi escrito.
            if let (Some(at), Some(original)) = (offered_at, original)
                && brought.len() == 1
            {
                original_of.insert(at, original);
            }
            for import in here_imports {
                let folder_only =
                    relative.is_some_and(|rule| matches!(rule.leading(&import.text), (count, "") if count > 0));
                if folder_only && !brought.is_empty() {
                    written.extend(
                        brought.iter().map(|name| Written { text: format!("{}{name}", import.text), ..import.clone() }),
                    );
                } else {
                    written.push(import);
                }
            }

            if let (Some((node, kind)), Some(name)) = (def, &name_text) {
                let header = decls.entry((node.start_byte(), name_byte, name.clone())).or_insert_with(|| Header {
                    kind: kind.to_string(),
                    pattern: m.pattern_index,
                    name: name.clone(),
                    node,
                    name_byte,
                    name_kind,
                    body_end: None,
                    value_start: None,
                    doc_inside: None,
                    owner: Vec::new(),
                    contract: Vec::new(),
                });
                // Two patterns may give the same declaration two kinds (a
                // `const` field is a field too): the one written first in the
                // query gives the kind, whatever order the matches come in.
                if m.pattern_index < header.pattern {
                    header.kind = kind.to_string();
                    header.pattern = m.pattern_index;
                }
                header.body_end = header.body_end.max(body_end);
                // Two patterns may write the same declaration's owner and its
                // contract apart: each name counts once.
                for (into, names) in [(&mut header.owner, &here_owner), (&mut header.contract, &here_contract)] {
                    for name in names {
                        if !into.contains(name) {
                            into.push(name.clone());
                        }
                    }
                }
                // Two patterns may give the same declaration: the earliest
                // value and the earliest inner documentation win.
                header.value_start = earliest(header.value_start, value_start);
                header.doc_inside = match (header.doc_inside.take(), doc_inside) {
                    (Some(a), Some(b)) => Some(if b.0 < a.0 { b } else { a }),
                    (a, b) => a.or(b),
                };
            }
            if let Some(name) = &name_text
                && !here_supers.is_empty() {
                    let key = simple_type_name(name).unwrap_or_else(|| name.clone());
                    let bucket = supers_by_name.entry(key).or_default();
                    for s in here_supers {
                        bucket.insert(s);
                    }
                }
        }

        // Where each declaration's own name is written: that name followed by
        // `(` is the header, not a call.
        let names_at: BTreeSet<usize> = decls.values().map(|h| h.name_byte).collect();
        // The node types the declarations of this file write their names
        // with: a node of one of them standing for a single word is a name.
        let name_kinds: BTreeSet<&str> = decls.values().map(|h| h.name_kind).collect();
        // Where each name of a statement that declares several is written,
        // by the byte the statement starts at: each of them keeps the part of
        // the header before the first name, and then only its own.
        let mut names_by_start: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (start, name_byte, _) in decls.keys() {
            names_by_start.entry(*start).or_default().push(*name_byte);
        }
        out.declarations = decls
            .into_values()
            .map(|h| {
                let shared = &names_by_start[&h.node.start_byte()];
                let split = (shared.len() > 1).then(|| Split {
                    first_name: shared[0],
                    name: h.name_byte,
                    next_name: shared.iter().copied().find(|&b| b > h.name_byte),
                });
                let key = simple_type_name(&h.name).unwrap_or_else(|| h.name.clone());
                let supertypes = supers_by_name
                    .get(&key)
                    .map(|s| s.iter().cloned().collect())
                    .unwrap_or_default();
                let above = doc_above(h.node, bytes, &decorations, self.doc_tags);
                let whole = match h.doc_inside {
                    Some((_, inside)) if above.whole.is_empty() => one_line(&inside, usize::MAX),
                    _ => above.whole,
                };
                let doc = one_line(&whole, DOC_MAX_CHARS);
                // A documentação inteira fica só quando o teto cortou, e só
                // quando o arquivo guarda o texto de dentro das peças.
                let whole_doc = if whole == doc || !keep.written_text { String::new() } else { whole };
                Decl {
                    kind: h.kind,
                    name: h.name,
                    line: above.first_row + 1,
                    end_line: (h.node.end_position().row + 1).max(h.body_end.unwrap_or(0)),
                    supertypes,
                    doc,
                    whole_doc,
                    body_comment: String::new(),
                    body_names: String::new(),
                    signature: signature_of(h.node, bytes, &decorations, h.value_start, split),
                    calls: Vec::new(),
                    used_by: Vec::new(),
                    common_calls: 0,
                    owner: h.owner,
                    contract: h.contract,
                    members: Vec::new(),
                    implements: Vec::new(),
                    implemented_by: Vec::new(),
                }
            })
            .collect();
        owners_in_file(&mut out.declarations);
        // O texto das linhas de cada declaração e os comentários do arquivo
        // saem de uma caminhada só pela árvore. O literal de texto e a
        // documentação escrita como literal não são código: os nomes escritos
        // neles ficam de fora.
        if keep.written_text {
            let not_code: HashSet<(usize, usize)> = literals
                .iter()
                .map(|(node, _)| (node.start_byte(), node.end_byte()))
                .chain(doc_spans.iter().copied())
                .collect();
            let written_text = WrittenText::of(root, bytes, self.doc_tags, &not_code);
            for decl in &mut out.declarations {
                (decl.body_comment, decl.body_names) = written_text.lines(decl.line, decl.end_line);
            }
            (out.file_doc, out.file_comment, out.file_doc_in_body) = written_text.of_file(&out.declarations);
        }
        if keep.texts_and_routes {
            out.texts =
                fixed_texts(&literals, bytes, &[&import_spans, &test_blocks, &doc_spans], &out.declarations, &self.name);
        }

        // The call sites and the citations of the file, minus the
        // declaration headers themselves (`foo` in `fn foo(` is where it is
        // defined, not a use of it) and minus what is written inside a
        // decoration, an import or a namespace name.
        let quiet: Spans = decorations.union(&import_spans).copied().collect();
        let heads;
        (out.calls, out.cites, heads) = use_sites(root, bytes, &comments, &quiet, &names_at, &name_kinds, &self.name);
        drop_local_uses(&out.declarations, &locals, &names_at, [&mut out.calls, &mut out.cites]);

        // Cada nome trazido é do import escrito no mesmo comando: o que fica
        // dentro do nó mais próximo, subindo a partir do nome, que contém
        // algum import. O caminho de uma chamada não traz nome.
        // O nome trazido por um repasse é também um nome que o arquivo
        // oferece, com o nome que tem no arquivo de origem.
        let in_module = |line: usize| out.module_lines.iter().any(|&(first, last)| (first..=last).contains(&line));
        let mut brought_by: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (node, name) in &imported_at {
            let mut at = Some(*node);
            while let Some(n) = at {
                let (start, end) = (n.start_byte(), n.end_byte());
                let mut inside = written.iter().filter(|w| w.call.is_none() && start <= w.byte && w.end <= end).peekable();
                if inside.peek().is_some() {
                    for w in inside {
                        brought_by.entry(w.text.clone()).or_default().insert(name.clone());
                        if w.reexport && !in_module(w.line) {
                            let original = original_of.get(&node.start_byte()).unwrap_or(name);
                            out.reexports.entry(w.text.clone()).or_default().insert(name.clone(), original.clone());
                        }
                    }
                    break;
                }
                at = n.parent();
            }
        }
        out.brought = brought_by.into_iter().map(|(import, names)| (import, names.into_iter().collect())).collect();
        // O repasse que não escreve nome nenhum oferece todos.
        for w in written.iter().filter(|w| w.reexport && !in_module(w.line)) {
            out.reexports.entry(w.text.clone()).or_insert_with(|| BTreeMap::from([("*".to_string(), "*".to_string())]));
        }

        // O import escrito ao menos uma vez dentro de um módulo do arquivo
        // guarda todas as linhas em que é escrito: é por elas que o grafo sabe
        // de quantos módulos cada uma sai antes de subir pasta.
        let nested: BTreeSet<&str> = written.iter().filter(|w| in_module(w.line)).map(|w| w.text.as_str()).collect();
        let mut import_lines: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for w in written.iter().filter(|w| nested.contains(w.text.as_str())) {
            import_lines.entry(w.text.clone()).or_default().push(w.line);
        }
        for lines in import_lines.values_mut() {
            lines.sort();
            lines.dedup();
        }
        out.import_lines = import_lines;
        // O import escrito dentro de um trecho de teste é do teste, e fica à
        // parte dos do arquivo. O caminho de chamada fora dele guarda as
        // chamadas escritas por ele.
        for Written { text, global, byte, call, .. } in written {
            if test_blocks.iter().any(|&(start, end)| (start..end).contains(&byte)) {
                out.test_imports.push(text);
                continue;
            }
            if let Some(site) = call {
                out.call_paths.entry(text.clone()).or_default().push(site);
            }
            if global {
                out.global_imports.push(text);
            } else {
                out.imports.push(text);
            }
        }
        out.imports.sort();
        out.imports.dedup();
        out.global_imports.sort();
        out.global_imports.dedup();
        out.test_imports.sort();
        out.test_imports.dedup();
        for (path, byte, call) in other_paths {
            if !test_blocks.iter().any(|&(start, end)| (start..end).contains(&byte)) {
                out.other_call_paths.entry(path).or_default().push(call);
            }
        }
        for sites in out.call_paths.values_mut().chain(out.other_call_paths.values_mut()) {
            sites.sort();
            sites.dedup();
        }
        // O nome que abre a cadeia de uma chamada — o qualificador sem nada
        // antes dele, ou a primeira parte do caminho dela — fica quando o
        // arquivo não o liga: nem nome local nem o próprio objeto. O caminho
        // aberto por um valor (`(await x).a.f()`) não tem nome no começo.
        let bound: BTreeSet<&str> = locals
            .iter()
            .map(|(_, _, name)| name.as_str())
            .chain(self_receivers(&self.name).iter().copied())
            .chain(parent_receivers(&self.name).iter().copied())
            .collect();
        let unbound: BTreeSet<&str> = heads
            .iter()
            .map(String::as_str)
            .chain(out.other_call_paths.keys().map(|path| first_segment(path, separators)))
            .filter(|head| is_identifier(head) && !bound.contains(head))
            .collect();
        out.unbound_heads = unbound.into_iter().map(str::to_string).collect();
        out.test_lines.sort();
        out.test_lines.dedup();
        out.module_lines.sort();
        out.module_lines.dedup();
        out.namespaces.sort();
        out.namespaces.dedup();
        if keep.texts_and_routes {
            let imports: Vec<String> = out.imports.iter().chain(&out.global_imports).cloned().collect();
            let found = routes::find(&self.routes, root, bytes, &imports, project, &out.declarations, &test_blocks);
            (out.routes, out.route_links) = (found.routes, found.links);
        }
        out
    }
}

/// Um import como foi escrito no arquivo: o texto já limpo, se ele vale para
/// mais arquivos que o que o escreve, o byte e a linha em que começa e, quando
/// é o caminho de uma chamada, a chamada escrita por ele.
#[derive(Clone)]
struct Written {
    text: String,
    global: bool,
    byte: usize,
    /// O byte em que o import escrito termina.
    end: usize,
    line: usize,
    call: Option<CallSite>,
    /// Escrito como repasse (`@reexport`).
    reexport: bool,
}

impl Written {
    fn at(text: String, global: bool, node: Node) -> Written {
        Written {
            text,
            global,
            byte: node.start_byte(),
            end: node.end_byte(),
            line: node.start_position().row + 1,
            call: None,
            reexport: false,
        }
    }
}

/// O último nome escrito antes da lista que contém o nó: `fs` em
/// `use std::fs::{self}`. `None` quando não há nome antes dela.
fn name_before_list(node: Node, bytes: &[u8]) -> Option<String> {
    let list = node.parent()?;
    let before = &bytes[..list.start_byte()];
    let word = |b: &u8| b.is_ascii_alphanumeric() || *b == b'_' || *b >= 0x80;
    let end = before.iter().rposition(word)? + 1;
    let start = before[..end].iter().rposition(|b| !word(b)).map_or(0, |i| i + 1);
    std::str::from_utf8(&before[start..end]).ok().filter(|name| is_identifier(name)).map(str::to_string)
}

/// Tira de `sites` o uso de um nome que o corpo da declaração em volta liga.
/// O nome escrito sozinho, numa linha depois daquela em que ele foi ligado —
/// a variável, o parâmetro, o nome novo de uma desestruturação — e até o fim
/// da declaração em volta dela (a de [`crate::graph::enclosing`]), também
/// dentro de uma declaração escrita ali, é esse valor, e não a declaração do
/// projeto de mesmo nome. Na própria linha do nome ligado, o uso fica: em
/// `let hoje = hoje(None);` a chamada é da função, e o nome só vale depois. A
/// mesma chamada numa declaração vizinha, sem o nome ligado, fica. O nome
/// ligado que é o nome de uma declaração (`names_at`) não conta: é a própria
/// declaração.
fn drop_local_uses(
    decls: &[Decl],
    locals: &[(usize, usize, String)],
    names_at: &BTreeSet<usize>,
    sites: [&mut Vec<CallSite>; 2],
) {
    // Cada nome ligado, com a linha em que foi ligado e a última da
    // declaração em volta (sem fim conhecido, até o fim do arquivo).
    let mut bound: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
    for (line, byte, name) in locals {
        if names_at.contains(byte) {
            continue;
        }
        let Some(di) = crate::graph::enclosing(decls, *line) else { continue };
        let last = match decls[di].end_line {
            0 => usize::MAX,
            end => end,
        };
        bound.entry(name.as_str()).or_default().push((*line, last));
    }
    if bound.is_empty() {
        return;
    }
    for list in sites {
        list.retain(|site| {
            let local = site.qualifier.is_empty()
                && bound
                    .get(site.name.as_str())
                    .is_some_and(|spans| spans.iter().any(|&(first, last)| first < site.line && site.line <= last));
            !local
        });
    }
}

/// A chamada do nome no nó, como [`use_sites`] a lê: o nome, a linha e o
/// qualificador escrito antes dele, pelos separadores de `lang`. `None` quando
/// o nó não é um nome.
fn called_site(node: Node, bytes: &[u8], comments: &Spans, lang: &str) -> Option<CallSite> {
    let name = node.utf8_text(bytes).ok().filter(|text| is_identifier(text))?;
    Some(CallSite {
        name: name.to_string(),
        line: node.start_position().row + 1,
        qualifier: qualifier_before(node, bytes, comments, lang),
    })
}

/// A declaration as the query gave it, before the supertypes captured
/// elsewhere in the file are attached to it.
struct Header<'t> {
    kind: String,
    /// The pattern that gave the kind: the first one of the query wins.
    pattern: usize,
    name: String,
    node: Node<'t>,
    /// Where the name capture starts, which tells the header from a call.
    name_byte: usize,
    /// The node type the name is written with.
    name_kind: &'static str,
    /// The last line of the body kept beside the declaration, when there is one.
    body_end: Option<usize>,
    /// Where the value the declaration is given starts, when a query marks it.
    value_start: Option<usize>,
    /// The documentation written inside the declaration (where it starts, and
    /// its text), when a query marks it.
    doc_inside: Option<(usize, String)>,
    /// O tipo dono escrito fora da declaração (`@owner`).
    owner: Vec<String>,
    /// O contrato que ela cumpre por onde foi escrita (`@owner.contract`).
    contract: Vec<String>,
}

/// Os donos de cada declaração no próprio arquivo: as que têm a faixa (da
/// primeira à última linha) contendo a dela, da mais interna para a mais
/// externa, antes do dono escrito fora dela. A mesma faixa não conta. Em ordem
/// de começo, e cada faixa aberta sai quando uma declaração começa depois do
/// fim dela: nenhuma declaração seguinte cabe mais nela.
fn owners_in_file(decls: &mut [Decl]) {
    let mut order: Vec<usize> = (0..decls.len()).collect();
    order.sort_by_key(|&i| (decls[i].line, std::cmp::Reverse(decls[i].end_line)));
    let mut open: Vec<usize> = Vec::new();
    let mut found: Vec<Vec<String>> = vec![Vec::new(); decls.len()];
    for i in order {
        let (line, end) = (decls[i].line, decls[i].end_line);
        open.retain(|&o| decls[o].end_line >= line);
        found[i] = open
            .iter()
            .rev()
            .map(|&o| &decls[o])
            .filter(|o| o.line <= line && end <= o.end_line && (o.line, o.end_line) != (line, end))
            .map(|o| o.name.clone())
            .collect();
        open.push(i);
    }
    for (decl, mut owners) in decls.iter_mut().zip(found) {
        for name in std::mem::take(&mut decl.owner) {
            if !owners.contains(&name) {
                owners.push(name);
            }
        }
        decl.owner = owners;
    }
}

/// Where the names of a statement that declares several are written: the
/// first of them, this declaration's own, and the next one after it, when
/// there is one.
#[derive(Clone, Copy)]
struct Split {
    first_name: usize,
    name: usize,
    next_name: Option<usize>,
}

/// The earlier of two optional positions; a missing one never wins.
fn earliest(a: Option<usize>, b: Option<usize>) -> Option<usize> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// The byte spans (start, end) of the decorations of a file.
type Spans = BTreeSet<(usize, usize)>;

fn is_decoration(node: &Node, decorations: &Spans) -> bool {
    decorations.contains(&(node.start_byte(), node.end_byte()))
}

/// How much of a documentation comment is kept. The map is read by machine,
/// never whole by a person, but a comment is prose: the first lines say what
/// the declaration is, and the rest is detail nobody searches by.
const DOC_MAX_CHARS: usize = 400;

/// How much of a signature is kept. The header ends at the body or at the
/// value, so what is this long is a parameter list, and a parameter list is
/// what tells one declaration from another: the ceiling keeps it whole in
/// all but the rarest case.
const SIGNATURE_MAX_CHARS: usize = 600;

/// The documentation comment written right above `node`: the `extra` nodes the
/// grammar attaches immediately above it (that is what a comment is in every
/// grammar), cleaned of their markers and joined into one line. A blank line
/// between the comment and the declaration ends the block — what is detached
/// from the declaration is not its documentation. Empty when there is none.
///
/// Two things stand between a comment and its declaration without breaking the
/// block. A decoration is passed over, and so is an unnamed token (a keyword)
/// until the first comment is joined. And when the siblings end with nothing
/// joined, the declaration is wrapped in another node, so the search goes on
/// above the wrapper.
///
/// The same path says where the declaration starts: at the first decoration
/// it passes over, in every language. A grammar that keeps the decoration
/// inside the node already starts the node there, and the line is the node's.
fn doc_above(node: Node, bytes: &[u8], decorations: &Spans, tags: &[&str]) -> Above {
    let mut parts: Vec<String> = Vec::new();
    let mut anchor = node;
    let mut top = node.start_position().row;
    let mut first_row = top;
    'climb: loop {
        let mut cur = anchor;
        while let Some(prev) = cur.prev_sibling() {
            if prev.end_position().row + 1 < top {
                break 'climb;
            }
            if prev.is_extra() {
                let Ok(text) = prev.utf8_text(bytes) else { break 'climb };
                parts.push(clean_comment(text, tags));
            } else if is_decoration(&prev, decorations) {
                first_row = first_row.min(prev.start_position().row);
            } else if prev.is_named() || !parts.is_empty() {
                break 'climb;
            }
            top = prev.start_position().row;
            cur = prev;
        }
        if !parts.is_empty() {
            break;
        }
        let Some(parent) = anchor.parent() else { break };
        top = top.min(parent.start_position().row);
        anchor = parent;
    }
    parts.reverse();
    Above { whole: one_line(&parts.join(" "), usize::MAX), first_row }
}

/// What is read above a declaration: its whole documentation comment, in one
/// line and with no ceiling, and the row (from zero) where the declaration
/// starts.
struct Above {
    whole: String,
    first_row: usize,
}

/// Um comentário ou um nome escrito no arquivo: a linha (a partir de zero)
/// em que começa, o byte e o texto — o comentário já limpo das marcas.
struct Piece {
    row: usize,
    byte: usize,
    text: String,
}

/// O que se lê numa caminhada pela árvore do arquivo, na ordem em que foi
/// escrito: cada comentário (os nós extras da gramática) e cada nome escrito
/// no código (a folha nomeada que se lê como nome), fora dos trechos que não
/// são código; e onde termina o comentário do começo do arquivo.
struct WrittenText {
    comments: Vec<Piece>,
    names: Vec<Piece>,
    /// O byte em que começa o primeiro nó do arquivo que não é comentário:
    /// o comentário antes dele é do começo do arquivo.
    first_code: usize,
}

impl WrittenText {
    /// Caminha pela árvore de `root` na ordem do texto. O comentário é lido
    /// inteiro, limpo das marcas e das `tags` de documentação; o nó cujo
    /// trecho está em `not_code` fica de fora, com o que tem dentro.
    fn of(root: Node, bytes: &[u8], tags: &[&str], not_code: &HashSet<(usize, usize)>) -> WrittenText {
        let mut comments = Vec::new();
        let mut names = Vec::new();
        let mut cursor = root.walk();
        let first_code = {
            let mut children = root.children(&mut cursor);
            children.find(|child| !child.is_extra()).map_or(usize::MAX, |child| child.start_byte())
        };
        let mut cursor = root.walk();
        loop {
            let node = cursor.node();
            let piece = |text: String| Piece { row: node.start_position().row, byte: node.start_byte(), text };
            let mut descend = false;
            if node.is_extra() {
                if let Ok(text) = node.utf8_text(bytes) {
                    let text = clean_comment(text, tags);
                    if !text.is_empty() {
                        comments.push(piece(text));
                    }
                }
            } else if !not_code.contains(&(node.start_byte(), node.end_byte())) {
                descend = node.child_count() > 0;
                if !descend && node.is_named()
                    && let Ok(text) = node.utf8_text(bytes)
                    && is_identifier(text)
                {
                    names.push(piece(text.to_string()));
                }
            }
            if descend && cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return WrittenText { comments, names, first_code };
                }
            }
        }
    }

    /// Os comentários e os nomes das linhas `first` a `last` (a partir de
    /// um), cada um numa linha: os comentários juntados com espaço, e cada
    /// nome uma vez, na ordem em que aparece. A declaração sem a última linha
    /// fica só com a primeira.
    fn lines(&self, first: usize, last: usize) -> (String, String) {
        let rows = rows_of(first, last);
        let comment: Vec<&str> = Self::within(&self.comments, &rows).iter().map(|piece| piece.text.as_str()).collect();
        let mut seen: HashSet<&str> = HashSet::new();
        let names: Vec<&str> = Self::within(&self.names, &rows)
            .iter()
            .map(|piece| piece.text.as_str())
            .filter(|name| seen.insert(name))
            .collect();
        (comment.join(" "), names.join(" "))
    }

    /// Os pedaços de `pieces`, que estão na ordem do texto, que começam numa
    /// das linhas `rows`.
    fn within<'p>(pieces: &'p [Piece], rows: &std::ops::Range<usize>) -> &'p [Piece] {
        let start = pieces.partition_point(|piece| piece.row < rows.start);
        let end = pieces.partition_point(|piece| piece.row < rows.end);
        &pieces[start..end.max(start)]
    }

    /// Os comentários do arquivo fora os das declarações, cada grupo numa
    /// linha: os do começo, e os outros que caem fora das linhas de toda
    /// declaração de `declarations` — os de dentro já estão no
    /// `body_comment` dela. E quantos bytes do começo do `body_comment` da
    /// primeira declaração de fora são comentários do começo do arquivo: o
    /// comentário escrito antes de todo código na primeira linha dela é das
    /// duas. Os comentários do começo são os primeiros do texto, e por isso
    /// os dela vêm primeiro no `body_comment`, cada um seguido de um espaço
    /// quando vem mais.
    fn of_file(&self, declarations: &[Decl]) -> (String, String, usize) {
        let spans: Vec<std::ops::Range<usize>> = declarations.iter().map(|d| rows_of(d.line, d.end_line)).collect();
        let (head, rest): (Vec<&Piece>, Vec<&Piece>) = self.comments.iter().partition(|piece| piece.byte < self.first_code);
        let outside: Vec<&Piece> = rest.into_iter().filter(|piece| !spans.iter().any(|rows| rows.contains(&piece.row))).collect();
        let lines: Vec<(usize, usize)> = declarations.iter().map(|d| (d.line, d.end_line)).collect();
        let in_body = outer_declarations(&lines).first().map_or(0, |&first| {
            let held = head.iter().filter(|piece| spans[first].contains(&piece.row));
            let bytes: usize = held.map(|piece| piece.text.len() + 1).sum();
            bytes.min(declarations[first].body_comment.len())
        });
        let joined = |pieces: Vec<&Piece>| pieces.iter().map(|piece| piece.text.as_str()).collect::<Vec<_>>().join(" ");
        (joined(head), joined(outside), in_body)
    }
}

/// As linhas de uma declaração da linha `first` à `last` (a partir de um),
/// como linhas da árvore (a partir de zero). A declaração sem a última linha
/// fica só com a primeira.
fn rows_of(first: usize, last: usize) -> std::ops::Range<usize> {
    first.saturating_sub(1)..last.max(first)
}

/// Drop the punctuation a comment is written with, line by line, and leave the
/// prose. Generic: the marker characters are the ones every comment syntax
/// draws its lines with, not a language's. The markup `tags` of the language
/// go too, their text staying (see [`strip_doc_tags`]).
fn clean_comment(raw: &str, tags: &[&str]) -> String {
    raw.lines()
        .map(|line| strip_doc_tags(line, tags))
        .map(|line| {
            let line = line.trim();
            let line = line.strip_suffix("*/").unwrap_or(line);
            line.trim_start_matches(['/', '*', '#', '-', ';', '!', '<', '=']).trim().to_string()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drop from `line` every markup tag named in `tags` — opening, closing or
/// self-closing — and keep the text between them. A self-closing tag that
/// carries a value leaves the value in its place: `<see cref="Base"/>` reads
/// `Base`. A `<...>` whose name is not in the list stays as written, since
/// elsewhere it is a type (`Option<usize>`). With no tags, the line is as it
/// came.
fn strip_doc_tags(line: &str, tags: &[&str]) -> String {
    if tags.is_empty() || !line.contains('<') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after = &rest[open..];
        match doc_tag_at(after, tags) {
            Some((len, value)) => {
                out.push_str(&value);
                rest = &after[len..];
            }
            None => {
                out.push('<');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The tag `text` opens with, when its name is one of `tags`: how many bytes
/// it takes, and what stays in its place (the first quoted value of a
/// self-closing tag, nothing otherwise).
fn doc_tag_at(text: &str, tags: &[&str]) -> Option<(usize, String)> {
    let inner = text.strip_prefix('<')?;
    let inner = inner.strip_prefix('/').unwrap_or(inner);
    let name_len = inner.find(|c: char| !c.is_alphanumeric()).unwrap_or(inner.len());
    let name = &inner[..name_len];
    if !tags.contains(&name) {
        return None;
    }
    let after_name = &inner[name_len..];
    if !after_name.starts_with(['>', '/', ' ', '\t']) {
        return None;
    }
    let close = after_name.find('>')?;
    let taken = text.len() - after_name.len() + close + 1;
    let attributes = &after_name[..close];
    let value = match attributes.strip_suffix('/') {
        Some(attributes) => quoted_value(attributes).unwrap_or_default(),
        None => String::new(),
    };
    Some((taken, value))
}

/// The first value written between quotes in `text`.
fn quoted_value(text: &str) -> Option<String> {
    let start = text.find(['"', '\''])?;
    let quote = text[start..].chars().next()?;
    let body = &text[start + 1..];
    let end = body.find(quote)?;
    Some(body[..end].to_string())
}

/// The declaration's own header: its text up to where the body opens — the
/// first `{` or `;` with no bracket open, a `{` right after `=>` even inside
/// brackets (`useCallback((a) => {`), or the first line break with nothing
/// left open. The body itself never comes: it was measured as the worst thing
/// to keep in the map. Neither does the value, when a query marks where it
/// starts (`value_start`): the header stops there, and the `=` left at its end
/// goes. It starts after the decorations and comments that open the node: an
/// attribute is not the header of what it adorns.
///
/// When the statement declares several names (`split`), each one's header is
/// what comes before the first name, followed by its own name up to its own
/// value or up to the next name: `export const a = 1, b = 2;` gives
/// `export const a` and `export const b`.
fn signature_of(
    node: Node,
    bytes: &[u8],
    decorations: &Spans,
    value_start: Option<usize>,
    split: Option<Split>,
) -> String {
    let mut start = node.start_byte();
    let mut walker = node.walk();
    for child in node.children(&mut walker) {
        if !(is_decoration(&child, decorations) || child.is_extra()) {
            start = child.start_byte();
            break;
        }
        start = child.end_byte();
    }
    let value = value_start.filter(|v| (start..=node.end_byte()).contains(v));
    let until = value.unwrap_or(node.end_byte());
    let own: Vec<u8> = match split.filter(|s| (start..until).contains(&s.first_name) && s.name < until) {
        Some(s) => {
            let own_end = s.next_name.map_or(until, |n| n.min(until));
            // What comes from the last comma before the next name on is not
            // part of this header (`, $` before `$q` in `public $p, $q;`).
            let mut own = &bytes[s.name..own_end];
            if s.next_name.is_some_and(|n| n <= until)
                && let Some(comma) = own.iter().rposition(|b| *b == b',')
            {
                own = &own[..comma];
            }
            let mut joined = bytes[start..s.first_name].to_vec();
            joined.extend_from_slice(own);
            joined
        }
        None => bytes[start..until].to_vec(),
    };
    let Ok(text) = std::str::from_utf8(&own) else { return String::new() };
    let mut depth: i32 = 0;
    let mut end = text.len();
    for (i, ch) in text.char_indices() {
        let opens_arrow_body = ch == '{' && text[..i].trim_end().ends_with("=>");
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '{' | ';' | '\n' if depth <= 0 || opens_arrow_body => {
                end = i;
                break;
            }
            _ => {}
        }
    }
    let mut head = &text[..end];
    if value.is_some() && end == text.len() {
        let trimmed = head.trim_end();
        head = trimmed.strip_suffix('=').unwrap_or(trimmed);
    }
    one_line(head, SIGNATURE_MAX_CHARS)
}

/// One line of text, whitespace collapsed, cut at `max` characters — on a word
/// boundary when there is one, so what is kept still reads.
fn one_line(text: &str, max: usize) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some((cut, _)) = joined.char_indices().nth(max) else { return joined };
    let cut = joined[..cut].rfind(' ').unwrap_or(cut);
    joined[..cut].trim_end().to_string()
}

/// O valor mais longo que se guarda de um texto fixo, em caracteres.
const TEXT_MAX_CHARS: usize = 300;

/// Quantos nós acima do literal a marca do texto é procurada: o bastante
/// para passar dos argumentos, da chamada que monta o texto e da que o
/// embrulha (`Err(Erro::Falta(format!("…")))`).
const MARK_LEVELS: usize = 6;

/// Quanto do trecho antes do filho, em bytes, a marca lê: o nome escrito logo
/// antes dele cabe aqui, e o nó que começa mais longe não abre com a palavra
/// que marca.
const MARK_BEFORE_BYTES: usize = 256;

/// Até quantos caracteres antes das aspas vai o prefixo de um literal
/// (`r#"`, `$@"`, `rb'`).
const QUOTE_PREFIX_MAX: usize = 3;

/// Os textos fixos do arquivo, em ordem de linha: de cada literal que não
/// cai em nenhum dos trechos de `skip` e tem cara de texto
/// ([`reads_as_text`]), o valor, a linha, a declaração que a contém e a
/// marca ([`mark_of`]) pelas chamadas de log e pelas formas de erro da
/// língua `lang`. O literal escrito sem aspas vale como está escrito; o
/// outro perde as aspas ([`literal_value`]).
fn fixed_texts(literals: &[(Node, bool)], bytes: &[u8], skip: &[&Spans], declarations: &[Decl], lang: &str) -> Vec<Text> {
    let (logs, errors) = (log_calls(lang), error_forms(lang));
    let overlaps = |node: &Node| {
        skip.iter().flat_map(|spans| spans.iter()).any(|&(start, end)| start < node.end_byte() && node.start_byte() < end)
    };
    let mut texts: Vec<Text> = literals
        .iter()
        .filter(|(node, _)| !overlaps(node))
        .filter_map(|(node, plain)| {
            let written = node.utf8_text(bytes).ok()?;
            let value = one_line(if *plain { written } else { literal_value(written) }, TEXT_MAX_CHARS);
            if !reads_as_text(&value) {
                return None;
            }
            let line = node.start_position().row + 1;
            let owner = crate::graph::enclosing(declarations, line).map(|at| declarations[at].name.clone());
            Some(Text { line, kind: mark_of(*node, bytes, logs, errors).to_string(), value, owner: owner.unwrap_or_default() })
        })
        .collect();
    texts.sort();
    texts.dedup();
    texts
}

/// O valor de um literal de texto sem as aspas e o que vem antes delas
/// (`r#"…"#`, `f'…'`, `@"…"`, `"""…"""`). O literal sem aspas perto do
/// começo fica como foi escrito.
pub(crate) fn literal_value(written: &str) -> &str {
    let Some(open) = written.find(['"', '\'', '`']).filter(|&at| at <= QUOTE_PREFIX_MAX) else { return written };
    let Some(quote) = written[open..].chars().next() else { return written };
    let run = written[open..].chars().take_while(|&c| c == quote).count();
    // Duas aspas seguidas só abrem o literal vazio; três ou mais abrem o de
    // várias linhas, que fecha com as mesmas.
    let count = if run >= 3 { run } else { 1 };
    let body = &written[open + count * quote.len_utf8()..];
    let body = if written[..open].contains('#') { body.trim_end_matches('#') } else { body };
    body.strip_suffix(quote.to_string().repeat(count).as_str()).unwrap_or(body)
}

/// O valor tem cara de texto: duas palavras ou mais — palavra é o trecho
/// entre espaços com duas letras ou mais —, ou forma de caminho ou chave:
/// sem espaço nem aspas, com `/`, `.`, `_` ou `-`, e com dois pedaços de duas
/// letras ou mais entre o que não é letra nem algarismo (`pedidos/novo`,
/// `erro.pedido_ausente`).
fn reads_as_text(value: &str) -> bool {
    let has_letters = |piece: &str| piece.chars().filter(|c| c.is_alphabetic()).count() >= 2;
    if value.split_whitespace().filter(|word| has_letters(word)).count() >= 2 {
        return true;
    }
    !value.contains(char::is_whitespace)
        && !value.contains(['"', '\'', '`'])
        && value.contains(['/', '.', '_', '-'])
        && value.split(|c: char| !c.is_alphanumeric()).filter(|piece| has_letters(piece)).count() >= 2
}

/// A marca de um texto fixo. Subindo a partir do literal, até
/// [`MARK_LEVELS`] nós, o primeiro nó cujo nome ([`names_before`]) casa com
/// uma das formas de erro `errors` dá [`TEXT_ERROR`], com uma das chamadas de
/// log `logs` dá [`TEXT_LOG`]; sem nenhum, [`TEXT_PLAIN`].
fn mark_of(literal: Node, bytes: &[u8], logs: &[&str], errors: &[&str]) -> &'static str {
    let mut child = literal;
    for _ in 0..MARK_LEVELS {
        let Some(parent) = child.parent() else { break };
        let names = names_before(&bytes[parent.start_byte()..child.start_byte()]);
        for (entries, mark) in [(errors, TEXT_ERROR), (logs, TEXT_LOG)] {
            if names.iter().any(|name| entries.iter().any(|entry| names_entry(name, entry))) {
                return mark;
            }
        }
        child = parent;
    }
    TEXT_PLAIN
}

/// Os nomes de um nó para a marca, lidos do trecho `before` que ele escreve
/// antes do filho que leva ao literal: o caminho escrito logo antes do filho
/// (`console.log` em `console.log("…")`, `tracing::info!`, `new Erro`) e,
/// quando o trecho é curto e fica numa linha só, a palavra que abre o nó
/// (`throw`, `raise`).
fn names_before(before: &[u8]) -> Vec<String> {
    let is_name = |c: char| c.is_alphanumeric() || c == '_';
    let in_path = |c: char| is_name(c) || ".:$\\!>-?".contains(c);
    let tail = String::from_utf8_lossy(&before[before.len().saturating_sub(MARK_BEFORE_BYTES)..]);
    let tail = tail.trim_end();
    let start = tail.char_indices().rev().take_while(|&(_, c)| in_path(c)).last().map_or(tail.len(), |(at, _)| at);
    let mut names = vec![tail[start..].trim_start_matches(|c: char| !is_name(c)).to_string()];
    if before.len() <= MARK_BEFORE_BYTES && !before.contains(&b'\n') {
        names.push(String::from_utf8_lossy(before).chars().take_while(|&c| is_name(c)).collect());
    }
    names.retain(|name| !name.is_empty());
    names
}

/// O nome casa com a entrada `entry` de `log_calls` ou de `error_forms`:
/// inteiro, ou a partir do começo de uma das partes dele (`info!` em
/// `tracing::info!`, `LogError` em `_logger.LogError`). O `*` da entrada casa
/// com letras, algarismos, `_` e `.`.
fn names_entry(name: &str, entry: &str) -> bool {
    let is_name = |c: char| c.is_alphanumeric() || c == '_';
    let mut before: Option<char> = None;
    for (at, c) in name.char_indices() {
        if is_name(c) && before.is_none_or(|b| !is_name(b)) && glob(entry, &name[at..]) {
            return true;
        }
        before = Some(c);
    }
    false
}

/// O texto casa com a entrada inteira, e o `*` dela com letras, algarismos,
/// `_` e `.`.
fn glob(entry: &str, text: &str) -> bool {
    match entry.split_once('*') {
        None => entry == text,
        Some((head, tail)) => {
            text.len() >= head.len() + tail.len()
                && text.starts_with(head)
                && text.ends_with(tail)
                && text[head.len()..text.len() - tail.len()].chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        }
    }
}

/// Every call the file makes and every name it cites without calling.
///
/// A call is a named leaf that reads as an identifier and is followed by an
/// opening parenthesis — the one shape a call has in every language we parse.
/// A citation is the same leaf not followed by a parenthesis: a type, a
/// constant or an enum member named in a parameter, a comparison or a path.
/// The letter a name starts with decides nothing, since how names are written
/// is a convention of each language: every such name is a candidate here, and
/// `graph` keeps only the ones that name a declaration the citing file sees.
///
/// Read off the tree, so what is inside a comment is never a use, and a
/// keyword never is either (it is not a named node). A citation right against
/// a quote is quoted text, not a name. The name is NOT resolved here: `graph`
/// does that with the whole project in hand, which is why what is stored
/// survives a pass that reads only the files that changed. What is written
/// right before the name, in `q::name` or `q.name`, is kept as its qualifier.
///
/// Nothing inside `quiet` is a use: not a decoration (an attribute calls
/// nothing), not an import (`Modules` in `using App.Modules;` is the path of
/// the import), not a name an import brings in (`limite` in
/// `import { limite } from`, whose use comes later in the file) and not a
/// namespace name. A declaration's own name, at
/// `names_at`, is its header.
///
/// A grammar may wrap a word in a name node (a keyword that is a name only in
/// some places). That node counts as a leaf when its one child spans exactly
/// what it spans, and only when it is of a type in `name_kinds`, the types the
/// declarations of the file write their names with: `x.from(1)` is a call,
/// and a modifier or a type keyword before a parenthesis is not.
///
/// O qualificador se lê pelos separadores de `lang` ([`qualifier_before`]).
/// Vêm junto os qualificadores que abrem a cadeia de uma chamada
/// ([`opens_chain`]).
fn use_sites(
    root: Node,
    bytes: &[u8],
    comments: &Spans,
    quiet: &Spans,
    names_at: &BTreeSet<usize>,
    name_kinds: &BTreeSet<&str>,
    lang: &str,
) -> (Vec<CallSite>, Vec<CallSite>, BTreeSet<String>) {
    let mut calls: BTreeSet<(usize, String, String)> = BTreeSet::new();
    let mut cites: BTreeSet<(usize, String, String)> = BTreeSet::new();
    let mut heads: BTreeSet<String> = BTreeSet::new();
    let mut cursor = root.walk();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if is_decoration(&node, quiet) || node.is_extra() {
            continue;
        }
        if node.child_count() > 0 && !is_wrapped_word(node, name_kinds) {
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
            continue;
        }
        if !node.is_named() || names_at.contains(&node.start_byte()) {
            continue;
        }
        let Ok(text) = node.utf8_text(bytes) else { continue };
        if !is_identifier(text) {
            continue;
        }
        let site = (node.start_position().row + 1, text.to_string(), qualifier_before(node, bytes, comments, lang));
        if followed_by_open_paren(node, bytes) {
            if opens_chain(node, bytes, comments, lang, &site.2) {
                heads.insert(site.2.clone());
            }
            calls.insert(site);
        } else if !against_a_quote(node, bytes) {
            cites.insert(site);
        }
    }
    let sites = |found: BTreeSet<(usize, String, String)>| {
        found.into_iter().map(|(line, name, qualifier)| CallSite { name, line, qualifier }).collect()
    };
    (sites(calls), sites(cites), heads)
}

/// O qualificador `qualifier`, escrito antes do nó, é um nome sem nada ligado
/// a ele por um separador de `lang` antes dele: abre a cadeia (`File` em
/// `File.ReadAllText(`, e não `repo` em `this.repo.salvar(` nem em
/// `f().repo.salvar(`).
fn opens_chain(node: Node, bytes: &[u8], comments: &Spans, lang: &str, qualifier: &str) -> bool {
    if qualifier.is_empty() || qualifier == RECEIVER {
        return false;
    }
    let separators = || qualifier_separators(lang).iter().chain(member_separators(lang));
    let before = code_before(bytes, comments, node.start_byte());
    let Some(before) = separators().find_map(|sep| before.strip_suffix(sep.as_bytes())) else { return false };
    let Some(before) = code_before(bytes, comments, before.len()).strip_suffix(qualifier.as_bytes()) else {
        return false;
    };
    let before = code_before(bytes, comments, before.len());
    !separators().any(|sep| before.ends_with(sep.as_bytes()))
}

/// Os trechos que a árvore marca como extras — os comentários —, sem descer
/// dentro deles.
fn comment_spans(root: Node) -> Spans {
    let mut spans = Spans::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.is_extra() {
            spans.insert((node.start_byte(), node.end_byte()));
        } else if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return spans;
            }
        }
    }
}

/// O código escrito antes do byte `at`, sem o espaço e os comentários
/// (`comments`) que vêm logo antes dele: o ponto que fecha a frase de um
/// comentário não liga o nome de depois ao que o comentário diz.
fn code_before<'b>(bytes: &'b [u8], comments: &Spans, at: usize) -> &'b [u8] {
    let mut end = at;
    loop {
        let before = bytes[..end].trim_ascii_end();
        let Some(last) = before.len().checked_sub(1) else { return before };
        match comments.range(..=(last, usize::MAX)).next_back() {
            Some(&(start, stop)) if last < stop && start < end => end = start,
            _ => return before,
        }
    }
}

/// A name node standing for one word: of a type in `name_kinds`, with a single
/// child that spans exactly what the node spans.
fn is_wrapped_word(node: Node, name_kinds: &BTreeSet<&str>) -> bool {
    node.child_count() == 1
        && node.is_named()
        && name_kinds.contains(node.kind())
        && node
            .child(0)
            .is_some_and(|c| c.child_count() == 0 && c.start_byte() == node.start_byte() && c.end_byte() == node.end_byte())
}

/// O que vem escrito antes do nó e ligado a ele por um separador de `lang`.
/// Pelo separador de nome qualificado, o nome antes dele: `preco` em
/// `crate::preco::total`, `model` em `model.User`. Pelo separador que só liga
/// método (`member_separators`), o nome só fica quando é o próprio objeto
/// (`self_receivers`): outro nome ali é um valor. Quando o que vem antes do
/// separador não é um nome (`f().total`, `a[0].total`, `...total`), ou é um
/// valor, a marca [`RECEIVER`]. Vazio quando o nó está sozinho. O comentário
/// escrito no meio não conta ([`code_before`]).
fn qualifier_before(node: Node, bytes: &[u8], comments: &Spans, lang: &str) -> String {
    let before = code_before(bytes, comments, node.start_byte());
    let strip = |separators: &[&str]| separators.iter().find_map(|sep| before.strip_suffix(sep.as_bytes()));
    let (before, only_member) = match (strip(qualifier_separators(lang)), strip(member_separators(lang))) {
        (Some(before), _) => (before, false),
        (None, Some(before)) => (before, true),
        (None, None) => return String::new(),
    };
    let before = code_before(bytes, comments, before.len());
    let start = before
        .iter()
        .rposition(|b| !(b.is_ascii_alphanumeric() || *b == b'_' || *b >= 0x80))
        .map_or(0, |i| i + 1);
    match std::str::from_utf8(&before[start..]) {
        Ok(q) if is_identifier(q) && (!only_member || self_receivers(lang).contains(&q)) => q.to_string(),
        _ => RECEIVER.to_string(),
    }
}

/// A primeira parte de um nome qualificado: o texto até o primeiro dos
/// `separators`, ou o texto inteiro quando não há nenhum.
/// O caminho sem os argumentos de tipo escritos nele (`<u8>` em
/// `crate::a::Caixa::<u8>`): cada trecho entre `<` e o `>` que o fecha sai, e
/// o separador que fica sem parte de um lado ou do outro sai junto.
fn without_type_arguments(path: &str, separators: &[&str]) -> String {
    if !path.contains('<') {
        return path.to_string();
    }
    let mut out = String::with_capacity(path.len());
    let mut depth = 0usize;
    for ch in path.chars() {
        match ch {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    for sep in separators.iter().filter(|sep| !sep.is_empty()) {
        let doubled = format!("{sep}{sep}");
        while out.contains(&doubled) {
            out = out.replace(&doubled, sep);
        }
        while let Some(rest) = out.strip_suffix(sep) {
            out = rest.to_string();
        }
    }
    out
}

fn first_segment<'a>(path: &'a str, separators: &[&str]) -> &'a str {
    let cut = separators.iter().filter_map(|sep| path.find(sep)).min().unwrap_or(path.len());
    &path[..cut]
}

/// The node touches a quote on either side: it is the text of a string, not a
/// name. Generic: every language quotes its text with one of these.
fn against_a_quote(node: Node, bytes: &[u8]) -> bool {
    let quote = |b: Option<&u8>| b.is_some_and(|b| matches!(b, b'"' | b'\'' | b'`'));
    quote(node.start_byte().checked_sub(1).and_then(|i| bytes.get(i))) || quote(bytes.get(node.end_byte()))
}

/// The next character after the node, whitespace apart, opens a parameter list.
fn followed_by_open_paren(node: Node, bytes: &[u8]) -> bool {
    bytes
        .get(node.end_byte()..)
        .and_then(|rest| rest.iter().find(|b| !b.is_ascii_whitespace()))
        .is_some_and(|b| *b == b'(')
}

/// The text reads as a name: letters, digits and underscores, starting with a
/// letter or an underscore. Two characters at least, the same floor
/// [`simple_type_name`] uses.
fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && text.chars().all(|c| c.is_alphanumeric() || c == '_')
        && text.chars().count() >= 2
}

/// Split a `.scm` source into top-level patterns and keep the ones that compile
/// against this grammar. Resilience over strictness: a query referencing a node
/// a given grammar version lacks drops that one pattern, not the language.
pub(crate) fn compile_good_patterns(lang: &Language, src: &str, name: &str) -> Vec<String> {
    split_patterns(src)
        .into_iter()
        .filter(|p| match Query::new(lang, p) {
            Ok(_) => true,
            Err(e) => {
                eprintln!("grain: '{name}' query pattern skipped ({e}): {}", first_line(p));
                false
            }
        })
        .collect()
}

/// Break a query into its top-level S-expression patterns. A pattern runs from a
/// `(`/`[` at depth 0 up to the next one, so trailing `@captures` (e.g. the
/// `@definition.class` after a closing paren) bundle with the preceding pattern.
fn split_patterns(src: &str) -> Vec<String> {
    let s = strip_comments(src);
    let mut starts = Vec::new();
    let mut depth: i32 = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' | '[' => {
                if depth == 0 {
                    starts.push(i);
                }
                depth += 1;
            }
            ')' | ']' => depth -= 1,
            _ => {}
        }
    }
    let mut out = Vec::new();
    for k in 0..starts.len() {
        let end = if k + 1 < starts.len() { starts[k + 1] } else { s.len() };
        let pat = s[starts[k]..end].trim().to_string();
        if !pat.is_empty() {
            out.push(pat);
        }
    }
    out
}

/// Drop `;`-to-end-of-line comments. The only string literals our queries hold
/// are the patterns of a `#match?`, which never carry a `;`, so a plain scan is
/// safe.
fn strip_comments(src: &str) -> String {
    src.lines()
        .map(|l| match l.find(';') {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s).trim()
}

/// Reduce a captured import/using statement to its path, dropping leading
/// keywords, quotes, and punctuation so it matches across languages.
fn clean_import(txt: &str) -> String {
    let mut s = txt.trim().to_string();
    for kw in ["global ", "using static ", "using ", "import ", "from "] {
        if let Some(rest) = s.strip_prefix(kw) {
            s = rest.trim().to_string();
        }
    }
    s.replace(['"', '\'', ';', '`'], "").trim().to_string()
}

/// `"A.B.EntityBase"` -> `"EntityBase"`; `"IServiceBase<A,B>"` ->
/// `"IServiceBase"`; `"std::fmt::Display"` -> `"Display"`. Returns `None` for
/// anything that reduces to fewer than two identifier characters (e.g. a bare
/// `<T>` generic-argument list captured by a wildcard).
fn simple_type_name(txt: &str) -> Option<String> {
    // Cut generic arguments / call parens first.
    let head = txt.split(['<', '(']).next().unwrap_or(txt).trim();
    // Take the last qualified segment.
    let tail = head.rsplit(['.', ' ', ':']).next().unwrap_or(head).trim();
    let name: String = tail.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.len() >= 2 {
        Some(name)
    } else {
        None
    }
}
