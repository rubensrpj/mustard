//! As rotas do servidor: o método HTTP, o caminho e a função que atende cada
//! uma. Cada framework é uma regra de dados — `routes/<framework>.toml`, com a
//! consulta em `routes/<framework>/*.scm` —, embutida pelo `build.rs`; este
//! módulo só junta o que a consulta captura, sem nome de framework nem de nó.
//!
//! A regra roda só no arquivo que liga o framework: pelo import dele, pelo
//! import global de outro arquivo que o alcança ou pela dependência do
//! manifesto mais próximo acima dele (ver [`Project`]). O que cada captura da
//! consulta diz:
//!
//! - `route.method`: o nome escrito do método, que a tabela `methods` da regra
//!   leva ao método HTTP; o nome fora da tabela não é rota.
//! - `route.method.grouped`: o mesmo, para o método que só é rota dentro de um
//!   `route.group`, de quem ele toma o caminho.
//! - `route.path`: o literal do caminho, no match do método ou do grupo. A
//!   rota sem ele fica no caminho dos prefixos.
//! - `route.handler`: o que atende a rota. Sem ele, quem atende é a declaração
//!   mais interna que contém a linha do método.
//! - `route.receiver`: o objeto em que a rota se registra.
//! - `route.group`: o trecho cujos métodos agrupados tomam o `route.path` do
//!   mesmo match.
//! - `route.prefix`: o literal de um prefixo. Vale para as rotas dentro do
//!   `route.scope` do mesmo match, ou para as do `route.target`: as
//!   registradas num objeto com esse nome, ou escritas numa declaração com
//!   esse nome, no mesmo arquivo.
//! - `route.class`: o nome da classe, que troca o marcador da regra
//!   (`class_marker`), sem o sufixo dela (`class_suffix`), nos caminhos das
//!   rotas dentro do `route.scope` do mesmo match.
//!
//! O caminho só conta quando é texto escrito ali: o montado numa variável não
//! casa com a captura de literal, e não faz rota.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Query, QueryCursor};

use crate::extract::{compile_good_patterns, literal_value};
use crate::graph::enclosing;
use crate::model::{Decl, Route, ANY_METHOD};

/// A regra de rota de um framework, como o `build.rs` a lê de
/// `routes/<framework>.toml` e da consulta dele.
pub(crate) struct RawRouteRule {
    pub framework: &'static str,
    /// As línguas do registro cujos arquivos a consulta lê.
    pub languages: &'static [&'static str],
    /// Os imports que ligam a regra no arquivo.
    pub imports: &'static [&'static str],
    /// As dependências de manifesto que ligam a regra nos arquivos sob o
    /// manifesto mais próximo acima deles que as declara.
    pub manifest_dependencies: &'static [&'static str],
    pub query: &'static str,
    /// O nome escrito do método e o método HTTP que ele diz.
    pub methods: &'static [(&'static str, &'static str)],
    /// O começo do pedaço de caminho que é parâmetro.
    pub param_prefixes: &'static [&'static str],
    /// As marcas que abrem e fecham um parâmetro.
    pub param_wrappers: &'static [(&'static str, &'static str)],
    /// O começo que faz o caminho da rota não se juntar aos prefixos dos
    /// trechos em que ela está (`route.scope`); o da montagem (`route.target`)
    /// continua.
    pub reset_marks: &'static [&'static str],
    /// O começo que todo caminho de rota tem no framework; o texto sem ele é
    /// outra coisa, e não faz rota. Vazio quando o caminho começa de
    /// qualquer jeito.
    pub path_starts: &'static [&'static str],
    pub class_marker: &'static str,
    pub class_suffix: &'static str,
}

// Traz `ROUTE_RULES`, gerada pelo `build.rs` dos arquivos de rotas.
include!(concat!(env!("OUT_DIR"), "/routes_generated.rs"));

/// O que uma captura da consulta de rotas diz; ver o topo do módulo.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Method,
    GroupedMethod,
    Path,
    Handler,
    Receiver,
    Group,
    Prefix,
    Scope,
    Class,
    Target,
    Ignore,
}

fn role(capture: &str) -> Role {
    match capture {
        "route.method" => Role::Method,
        "route.method.grouped" => Role::GroupedMethod,
        "route.path" => Role::Path,
        "route.handler" => Role::Handler,
        "route.receiver" => Role::Receiver,
        "route.group" => Role::Group,
        "route.prefix" => Role::Prefix,
        "route.scope" => Role::Scope,
        "route.class" => Role::Class,
        "route.target" => Role::Target,
        _ => Role::Ignore,
    }
}

/// A regra de um framework para uma língua. A consulta se compila na
/// gramática da língua uma vez só, na primeira vez que a regra liga num
/// arquivo: a passada sem nenhum arquivo do framework não a compila.
pub(crate) struct RouteRule {
    raw: &'static RawRouteRule,
    lang: &'static str,
    language: Language,
    /// A consulta compilada, depois da primeira vez que a regra ligou;
    /// `None` dentro quando nenhum padrão dela compila.
    compiled: OnceLock<Option<Compiled>>,
}

/// A consulta de uma regra, compilada, e o papel de cada captura dela.
struct Compiled {
    query: Query,
    /// `roles[i]` é o papel da captura `i` da consulta.
    roles: Vec<Role>,
}

/// As regras dos frameworks escritos na língua `lang`, ainda sem compilar.
pub(crate) fn rules_for(lang: &'static str, language: &Language) -> Vec<RouteRule> {
    ROUTE_RULES
        .iter()
        .filter(|raw| raw.languages.contains(&lang))
        .map(|raw| RouteRule { raw, lang, language: language.clone(), compiled: OnceLock::new() })
        .collect()
}

/// O que o projeto diz de um arquivo e liga nele a regra de um framework sem
/// import próprio.
#[derive(Clone, Copy, Default)]
pub(crate) struct Project<'a> {
    /// Os imports globais escritos em outros arquivos que o alcançam.
    pub global_imports: &'a [String],
    /// As dependências do manifesto mais próximo acima dele.
    pub manifest_deps: &'a [String],
}

/// Alguma das regras liga no arquivo que importa `imports` com o que o
/// projeto diz em `more`, e não ligava só com o que ele diz em `less`.
pub(crate) fn turned_on_by(rules: &[RouteRule], imports: &[String], less: &Project, more: &Project) -> bool {
    rules.iter().any(|rule| rule.active(imports, more) && !rule.active(imports, less))
}

/// As rotas do arquivo, pelas regras que ele liga com os `imports` dele ou
/// com o que o `project` diz dele, fora as escritas num trecho de `skip`, em
/// ordem.
pub(crate) fn find(
    rules: &[RouteRule],
    root: Node,
    bytes: &[u8],
    imports: &[String],
    project: &Project,
    declarations: &[Decl],
    skip: &BTreeSet<(usize, usize)>,
) -> Vec<Route> {
    let mut routes: Vec<Route> = rules
        .iter()
        .filter(|rule| rule.active(imports, project))
        .filter_map(|rule| Some((rule, rule.compiled()?)))
        .flat_map(|(rule, compiled)| rule.routes(compiled, root, bytes, declarations, skip))
        .collect();
    routes.sort();
    routes.dedup();
    routes
}

/// As capturas de um match, pelo papel.
#[derive(Default)]
struct Captured<'t> {
    method: Option<Node<'t>>,
    grouped: bool,
    path: Option<Node<'t>>,
    handler: Option<Node<'t>>,
    receiver: Option<Node<'t>>,
    group: Option<Node<'t>>,
    prefix: Option<Node<'t>>,
    scope: Option<Node<'t>>,
    class: Option<Node<'t>>,
    target: Option<Node<'t>>,
}

/// Um prefixo e a quem ele vale: às rotas dentro do trecho `scope`, ou às do
/// nome `target`.
struct Prefix {
    value: String,
    scope: Option<(usize, usize)>,
    class: Option<String>,
    target: Option<String>,
}

/// Uma rota antes da junção: se o caminho foi escrito nela, para juntar a de
/// qualquer método com caminho à de método conhecido sem caminho.
struct Built {
    route: Route,
    has_path: bool,
}

impl RouteRule {
    /// A consulta da regra, compilada na primeira vez que se pede. O padrão
    /// que não compila cai sozinho, com o aviso de sempre, dado então.
    fn compiled(&self) -> Option<&Compiled> {
        self.compiled
            .get_or_init(|| {
                let good = compile_good_patterns(&self.language, self.raw.query, &self.name());
                if good.is_empty() {
                    return None;
                }
                let query = Query::new(&self.language, &good.join("\n")).ok()?;
                let roles = query.capture_names().iter().map(|name| role(name)).collect();
                Some(Compiled { query, roles })
            })
            .as_ref()
    }

    /// O nome da regra: o framework e a língua, `framework/língua`.
    pub(crate) fn name(&self) -> String {
        format!("{}/{}", self.raw.framework, self.lang)
    }

    /// A regra liga também pela dependência do manifesto.
    pub(crate) fn follows_manifest(&self) -> bool {
        !self.raw.manifest_dependencies.is_empty()
    }

    /// A consulta da regra já foi compilada nesta passada.
    pub(crate) fn was_compiled(&self) -> bool {
        self.compiled.get().is_some()
    }

    /// O arquivo liga o framework: ele ou um import global que o alcança o
    /// importa, ou o manifesto mais próximo acima dele o declara.
    fn active(&self, imports: &[String], project: &Project) -> bool {
        self.imported(imports)
            || self.imported(project.global_imports)
            || self.raw.manifest_dependencies.iter().any(|dep| project.manifest_deps.iter().any(|has| has == dep))
    }

    /// Um dos `imports` é um nome da regra, ou começa por ele seguido de
    /// separador. Letra, algarismo, `_` ou `-` logo depois fazem outro nome,
    /// que não liga a regra.
    fn imported(&self, imports: &[String]) -> bool {
        imports.iter().any(|import| {
            self.raw.imports.iter().any(|name| {
                import.strip_prefix(name).is_some_and(|rest| {
                    rest.chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-'))
                })
            })
        })
    }

    fn method(&self, written: &str) -> Option<&'static str> {
        self.raw.methods.iter().find(|(name, _)| *name == written).map(|(_, method)| *method)
    }

    fn routes(
        &self,
        compiled: &Compiled,
        root: Node,
        bytes: &[u8],
        declarations: &[Decl],
        skip: &BTreeSet<(usize, usize)>,
    ) -> Vec<Route> {
        let text = |node: Node| node.utf8_text(bytes).unwrap_or_default().to_string();
        let literal = |node: Node| literal_value(&text(node)).to_string();
        let span = |node: Node| (node.start_byte(), node.end_byte());

        let mut found: Vec<Captured> = Vec::new();
        let mut groups: Vec<((usize, usize), String)> = Vec::new();
        let mut prefixes: Vec<Prefix> = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&compiled.query, root, bytes);
        while let Some(m) = matches.next() {
            let mut here = Captured::default();
            for cap in m.captures {
                let node = Some(cap.node);
                match compiled.roles[cap.index as usize] {
                    Role::Method => here.method = node,
                    Role::GroupedMethod => (here.method, here.grouped) = (node, true),
                    Role::Path => here.path = node,
                    Role::Handler => here.handler = node,
                    Role::Receiver => here.receiver = node,
                    Role::Group => here.group = node,
                    Role::Prefix => here.prefix = node,
                    Role::Scope => here.scope = node,
                    Role::Class => here.class = node,
                    Role::Target => here.target = node,
                    Role::Ignore => {}
                }
            }
            if here.method.is_some() {
                found.push(here);
            } else if let (Some(group), Some(path)) = (here.group, here.path) {
                groups.push((span(group), literal(path)));
            } else if let Some(prefix) = here.prefix {
                prefixes.push(Prefix {
                    value: literal(prefix),
                    scope: here.scope.map(span),
                    class: here.class.map(text),
                    target: here.target.map(text),
                });
            }
        }

        let mut built: Vec<Built> = Vec::new();
        for here in found {
            let Some(at) = here.method else { continue };
            let byte = at.start_byte();
            if skip.iter().any(|&(start, end)| (start..end).contains(&byte)) {
                continue;
            }
            let Some(method) = self.method(&text(at)) else { continue };
            let inside = |(start, end): (usize, usize)| start <= byte && byte < end;
            let own = if here.grouped {
                let group = groups.iter().filter(|(span, _)| inside(*span)).min_by_key(|((start, end), _)| end - start);
                let Some((_, path)) = group else { continue };
                Some(path.clone())
            } else {
                here.path.map(literal)
            };
            let line = at.start_position().row + 1;
            let owner = enclosing(declarations, line).map(|i| declarations[i].name.as_str());
            let receiver = here.receiver.map(text);
            let mounted = prefixes.iter().filter(|p| {
                p.target.as_deref().is_some_and(|target| Some(target) == receiver.as_deref() || Some(target) == owner)
            });
            let mut scoped: Vec<&Prefix> = prefixes.iter().filter(|p| p.scope.is_some_and(inside)).collect();
            scoped.sort_by_key(|p| p.scope.map(|(start, end)| (start, std::cmp::Reverse(end))));
            let class = scoped.iter().rev().find_map(|p| p.class.as_deref());

            let path = own.clone().unwrap_or_default();
            if !self.raw.path_starts.is_empty() && !self.raw.path_starts.iter().any(|start| path.starts_with(start)) {
                continue;
            }
            let reset = self.raw.reset_marks.iter().find(|mark| path.starts_with(**mark));
            let mut pieces: Vec<&str> = mounted.map(|p| p.value.as_str()).collect();
            if reset.is_none() {
                pieces.extend(scoped.iter().map(|p| p.value.as_str()));
            }
            pieces.push(reset.map_or(path.as_str(), |mark| &path[mark.len()..]));
            let written =
                pieces.iter().map(|piece| piece.trim_matches('/')).filter(|piece| !piece.is_empty()).collect::<Vec<_>>().join("/");
            let (handler, handler_line) = handler_of(here.handler, bytes, declarations, line);
            built.push(Built {
                route: Route {
                    method: method.to_string(),
                    path: self.normalized(&written, class),
                    written,
                    handler,
                    line: handler_line,
                    framework: self.raw.framework.to_string(),
                },
                has_path: own.is_some_and(|path| !path.is_empty()),
            });
        }
        joined(built)
    }

    /// O caminho padronizado: o marcador trocado pelo nome da classe sem o
    /// sufixo, as barras de sobra fora, cada parâmetro como `{}` e o resto em
    /// minúsculas.
    fn normalized(&self, written: &str, class: Option<&str>) -> String {
        let marked = match class {
            Some(class) if !self.raw.class_marker.is_empty() => {
                let name = class.strip_suffix(self.raw.class_suffix).filter(|name| !name.is_empty()).unwrap_or(class);
                written.replace(self.raw.class_marker, name)
            }
            _ => written.to_string(),
        };
        marked.split('/').filter(|piece| !piece.is_empty()).map(|piece| self.piece(piece)).collect::<Vec<_>>().join("/")
    }

    /// Um pedaço do caminho padronizado: o que começa por um prefixo de
    /// parâmetro é todo `{}`; cada trecho entre as marcas de um parâmetro vira
    /// `{}`; o resto fica em minúsculas.
    fn piece(&self, piece: &str) -> String {
        if self.raw.param_prefixes.iter().any(|prefix| piece.starts_with(prefix)) {
            return PARAM.to_string();
        }
        let mut out = String::new();
        let mut rest = piece;
        loop {
            let next = self
                .raw
                .param_wrappers
                .iter()
                .filter_map(|(open, close)| {
                    let at = rest.find(open)?;
                    let end = at + open.len() + rest[at + open.len()..].find(close)? + close.len();
                    Some((at, end))
                })
                .min();
            let Some((at, end)) = next else {
                out.push_str(&rest.to_lowercase());
                return out;
            };
            out.push_str(&rest[..at].to_lowercase());
            out.push_str(PARAM);
            rest = &rest[end..];
        }
    }
}

/// Como o caminho padronizado escreve um parâmetro.
const PARAM: &str = "{}";

/// A rota de qualquer método com caminho e a de método conhecido sem caminho,
/// da mesma função com nome, são uma rota só: o método de uma no caminho da
/// outra.
fn joined(built: Vec<Built>) -> Vec<Route> {
    let same = |a: &Route, b: &Route| !a.handler.is_empty() && a.handler == b.handler && a.line == b.line;
    let any_with_path = |b: &Built| b.route.method == ANY_METHOD && b.has_path;
    let known_without_path = |b: &Built| b.route.method != ANY_METHOD && !b.has_path;
    let mut routes = Vec::new();
    for b in &built {
        if any_with_path(b) && built.iter().any(|o| known_without_path(o) && same(&o.route, &b.route)) {
            continue;
        }
        let paths: Vec<&Route> = if known_without_path(b) {
            built.iter().filter(|o| any_with_path(o) && same(&o.route, &b.route)).map(|o| &o.route).collect()
        } else {
            Vec::new()
        };
        if paths.is_empty() {
            routes.push(b.route.clone());
        }
        for with_path in paths {
            routes.push(Route { method: b.route.method.clone(), ..with_path.clone() });
        }
    }
    routes
}

/// Quem atende a rota. O nome escrito em `handler` — o último nome dele,
/// quando ele é um caminho (`handlers::ler`) — vai com a linha da declaração
/// com esse nome no arquivo, a que contém o lugar em que ele foi escrito
/// primeiro; sem declaração, com a linha em que foi escrito. A função escrita
/// ali mesmo fica sem nome, na linha dela. Sem `handler`, a declaração mais
/// interna que contém a linha `line` do método; fora de toda declaração, sem
/// nome, nessa linha.
fn handler_of(handler: Option<Node>, bytes: &[u8], declarations: &[Decl], line: usize) -> (String, usize) {
    let Some(node) = handler else {
        return enclosing(declarations, line).map_or((String::new(), line), |i| (declarations[i].name.clone(), declarations[i].line));
    };
    let at = node.start_position().row + 1;
    let Some(name) = written_name(node.utf8_text(bytes).unwrap_or_default()) else { return (String::new(), at) };
    let named = || declarations.iter().filter(|d| d.name == name);
    let found = named().find(|d| d.line <= at && at <= d.end_line).or_else(|| named().next());
    let line = found.map_or(at, |d| d.line);
    (name, line)
}

/// O último nome de um texto feito só de nomes e dos separadores `.` e `:`
/// (`ler`, `this.criar`, `handlers::ler`); `None` para o que é outra coisa,
/// como a função escrita ali mesmo.
fn written_name(text: &str) -> Option<String> {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    if text.is_empty() || !text.chars().all(|c| word(c) || c == '.' || c == ':') {
        return None;
    }
    text.rsplit(|c: char| !word(c)).next().filter(|name| !name.is_empty()).map(str::to_string)
}
