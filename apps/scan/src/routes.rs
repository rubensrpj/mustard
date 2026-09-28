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
//! - `route.method.text`: o método escrito como texto (`"POST"`), que vale
//!   sem as aspas, em maiúsculas; cada texto é uma rota.
//! - `route.path`: o literal do caminho, no match do método ou do grupo. A
//!   rota sem ele fica no caminho dos prefixos. Na regra com método padrão
//!   (`default_method`), o match que captura o caminho sem método é uma rota
//!   desse método, a menos que outro match do mesmo caminho escrito diga o
//!   método dele.
//! - `route.handler`: o que atende a rota. Sem ele, quem atende é a declaração
//!   mais interna que contém a linha do método.
//! - `route.receiver`: o objeto em que a rota se registra; no match de
//!   `route.nest` e no de `route.call`, o objeto escrito antes do nome.
//! - `route.group`: o trecho cujos métodos agrupados tomam o `route.path` do
//!   mesmo match.
//! - `route.prefix`: o literal de um prefixo. Vale para as rotas dentro do
//!   `route.scope` do mesmo match, ou para as do `route.target`: as
//!   registradas num objeto com esse nome, ou escritas numa declaração com
//!   esse nome, no mesmo arquivo. O `route.target` que não alcança rota
//!   nenhuma do arquivo é um nome trazido de outro: o prefixo vale para as
//!   rotas do arquivo de onde ele vem (ver [`across_files`]).
//! - `route.class`: o nome da classe, que troca o marcador da regra
//!   (`class_marker`), sem o sufixo dela (`class_suffix`), nos caminhos das
//!   rotas dentro do `route.scope` do mesmo match.
//! - `route.module`: o nome que recebe um módulo inteiro. Montado, ele leva o
//!   prefixo a todas as rotas do arquivo de onde vem, e não só às
//!   registradas num objeto com o nome dele.
//! - `route.target.module`: o alvo de um prefixo escrito como texto que
//!   nomeia um módulo (`include('loja.urls')`). Ele se lê como import do
//!   arquivo que o escreve, e o prefixo vale para todas as rotas do arquivo
//!   que ele nomeia.
//! - `route.prefix.global`: o literal do prefixo de todas as rotas do
//!   framework no projeto do arquivo que o escreve.
//! - `route.exclude`: o caminho de uma rota que o prefixo global do mesmo
//!   match não alcança; `route.exclude.method`, o método dela, sem o qual
//!   vale qualquer um; `route.exclude.item`, o trecho que junta o caminho e o
//!   método escritos em matches diferentes. A exclusão que termina numa das
//!   marcas `exclude_wildcards` da regra vale para toda rota que começa pelo
//!   resto.
//! - `route.nest`: a chamada que faz do objeto dela (`route.receiver`) um
//!   grupo, com mais o `route.prefix`. O valor que começa por ela é o grupo,
//!   em qualquer ponto da cadeia; ele vale como objeto de uma rota, de outro
//!   grupo ou de uma chamada, e na variável que o guarda (`route.variable`,
//!   com o valor em `route.value`), dentro da mesma declaração.
//! - `route.parameter`: o parâmetro de uma declaração que pode levar um
//!   grupo, com o nome em `route.parameter.name`; `route.parameter.receiver`,
//!   o que recebe o objeto escrito antes do nome na chamada, que fica fora da
//!   contagem dos outros. O motor conta a posição de cada um entre os irmãos
//!   do mesmo tipo. A rota cujo objeto é um deles guarda o prefixo em aberto
//!   ([`crate::model::Route::open`]).
//! - `route.call`: o nome chamado. O objeto antes dele (`route.receiver`) ou o
//!   argumento (`route.argument`) que é grupo entrega o prefixo dele à
//!   declaração com esse nome, na posição do receptor ou na do argumento
//!   ([`crate::model::Handoff`]). O nome que faz rota não entrega nada.
//!
//! O caminho só conta quando é texto escrito ali: o montado numa variável não
//! casa com a captura de literal, e não faz rota.
//!
//! A regra da tela acha as chamadas que a tela faz às rotas do servidor
//! ([`crate::model::RouteCall`]), e não rotas. Ela liga como a do servidor,
//! ou em todo arquivo das línguas dela, quando a regra diz que é global. As
//! capturas dela:
//!
//! - `client.method`: o nome chamado, que a tabela `methods` leva ao método
//!   HTTP (`get` em `api.get('/pedidos')`); o nome fora da tabela não é
//!   chamada.
//! - `client.path`: o literal do caminho. O que começa por um parâmetro é
//!   montado sobre um valor, como a base guardada numa variável, e não liga.
//! - `client.option`: o método escrito nas opções da chamada, que vale no
//!   lugar do que o nome diz.
//! - `client.receiver`: o objeto antes do nome. A chamada conta quando ele é
//!   o objeto da biblioteca — um nome que o import dela traz —, um cliente
//!   feito no arquivo ou um nome trazido de outro arquivo, cujo cliente a
//!   ligação acha lá. Sem objeto, a chamada solta conta.
//! - `client.made`: a chamada que faz um cliente, com o objeto que a faz em
//!   `client.factory` — que precisa ser o da biblioteca —, o nome que o
//!   recebe em `client.instance` e o literal da base em `client.base`. Sem
//!   `client.instance`, o cliente é o que o arquivo exporta como padrão.
//!
//! Uma regra é do servidor ou da tela: a consulta dela usa as capturas de um
//! lado só.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Query, QueryCursor};

use crate::extract::{compile_good_query, literal_value};
use crate::graph::{enclosing, is_under, project_dir};
use crate::model::{
    Client, Decl, Exclusion, GlobalPrefix, Handoff, Manifest, Module, Mount, OpenPrefix, Route, RouteCall, RouteLinks,
    RoutePath, ANY_METHOD,
};

/// A regra de rota de um framework, como o `build.rs` a lê de
/// `routes/<framework>.toml` e da consulta dele.
pub(crate) struct RawRouteRule {
    pub framework: &'static str,
    /// As línguas do registro cujos arquivos a consulta lê.
    pub languages: &'static [&'static str],
    /// A regra liga em todo arquivo das línguas dela, sem import.
    pub global: bool,
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
    /// O fim que faz a exclusão do prefixo global valer para toda rota que
    /// começa pelo resto dela.
    pub exclude_wildcards: &'static [&'static str],
    pub class_marker: &'static str,
    pub class_suffix: &'static str,
    /// O método da rota em que a consulta não captura método nenhum; vazio
    /// quando toda rota da regra captura o dela.
    pub default_method: &'static str,
    /// As marcas que saem das pontas do caminho escrito antes de tudo.
    pub path_trims: &'static [&'static str],
}

// Traz `ROUTE_RULES`, gerada pelo `build.rs` dos arquivos de rotas.
include!(concat!(env!("OUT_DIR"), "/routes_generated.rs"));

/// O que uma captura da consulta de rotas diz; ver o topo do módulo.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Method,
    GroupedMethod,
    MethodText,
    Path,
    Handler,
    Receiver,
    Group,
    Prefix,
    Scope,
    Class,
    Target,
    TargetModule,
    Module,
    GlobalPrefix,
    Exclude,
    ExcludeMethod,
    ExcludeItem,
    Nest,
    Variable,
    Value,
    Parameter,
    ParameterName,
    ParameterReceiver,
    Call,
    Argument,
    ClientMethod,
    ClientPath,
    ClientOption,
    ClientReceiver,
    ClientMade,
    ClientFactory,
    ClientInstance,
    ClientBase,
    Ignore,
}

impl Role {
    /// A captura é do lado da tela.
    fn is_client(self) -> bool {
        matches!(
            self,
            Self::ClientMethod
                | Self::ClientPath
                | Self::ClientOption
                | Self::ClientReceiver
                | Self::ClientMade
                | Self::ClientFactory
                | Self::ClientInstance
                | Self::ClientBase
        )
    }
}

fn role(capture: &str) -> Role {
    match capture {
        "route.method" => Role::Method,
        "route.method.grouped" => Role::GroupedMethod,
        "route.method.text" => Role::MethodText,
        "route.path" => Role::Path,
        "route.handler" => Role::Handler,
        "route.receiver" => Role::Receiver,
        "route.group" => Role::Group,
        "route.prefix" => Role::Prefix,
        "route.scope" => Role::Scope,
        "route.class" => Role::Class,
        "route.target" => Role::Target,
        "route.target.module" => Role::TargetModule,
        "route.module" => Role::Module,
        "route.prefix.global" => Role::GlobalPrefix,
        "route.exclude" => Role::Exclude,
        "route.exclude.method" => Role::ExcludeMethod,
        "route.exclude.item" => Role::ExcludeItem,
        "route.nest" => Role::Nest,
        "route.variable" => Role::Variable,
        "route.value" => Role::Value,
        "route.parameter" => Role::Parameter,
        "route.parameter.name" => Role::ParameterName,
        "route.parameter.receiver" => Role::ParameterReceiver,
        "route.call" => Role::Call,
        "route.argument" => Role::Argument,
        "client.method" => Role::ClientMethod,
        "client.path" => Role::ClientPath,
        "client.option" => Role::ClientOption,
        "client.receiver" => Role::ClientReceiver,
        "client.made" => Role::ClientMade,
        "client.factory" => Role::ClientFactory,
        "client.instance" => Role::ClientInstance,
        "client.base" => Role::ClientBase,
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
    /// A regra é da tela: acha chamadas, e não rotas.
    client: bool,
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

/// O que as regras acham num arquivo: as rotas, os prefixos que ele escreve
/// para rotas de outros arquivos e as chamadas que a tela faz às rotas.
#[derive(Default)]
pub(crate) struct Found {
    pub routes: Vec<Route>,
    pub links: RouteLinks,
    pub calls: Vec<RouteCall>,
}

/// O que o arquivo diz de si às regras: os `imports` dele e, de cada um, os
/// nomes que ele traz (`brought`), as declarações e os trechos de teste
/// (`skip`), cujas rotas e chamadas não contam.
pub(crate) struct Source<'s> {
    pub imports: &'s [String],
    pub brought: &'s BTreeMap<String, Vec<String>>,
    pub declarations: &'s [Decl],
    pub skip: &'s BTreeSet<(usize, usize)>,
}

/// As rotas do arquivo, pelas regras que ele liga com os imports dele ou
/// com o que o `project` diz dele, em ordem, os prefixos que ele escreve
/// para rotas de outros arquivos e as chamadas da tela escritas nele.
pub(crate) fn find(rules: &[RouteRule], root: Node, bytes: &[u8], source: &Source, project: &Project) -> Found {
    let (declarations, skip) = (source.declarations, source.skip);
    let mut found = Found::default();
    for (rule, compiled) in rules
        .iter()
        .filter(|rule| rule.active(source.imports, project) && rule.may_find(bytes))
        .filter_map(|rule| Some((rule, rule.compiled()?)))
    {
        if compiled.client {
            let (calls, clients) = rule.calls(compiled, root, bytes, source);
            found.calls.extend(calls);
            found.links.clients.extend(clients);
            continue;
        }
        let (routes, links) = rule.routes(compiled, root, bytes, declarations, skip);
        found.routes.extend(routes);
        found.links.mounts.extend(links.mounts);
        found.links.globals.extend(links.globals);
        found.links.handoffs.extend(links.handoffs);
    }
    found.routes.sort();
    found.routes.dedup();
    found.links.mounts.sort();
    found.links.mounts.dedup();
    found.links.globals.sort();
    found.links.globals.dedup();
    found.links.handoffs.sort();
    found.links.handoffs.dedup();
    found.links.clients.sort();
    found.links.clients.dedup();
    found.calls.sort();
    found.calls.dedup();
    found
}

/// As capturas de um match, pelo papel.
#[derive(Default)]
struct Captured<'t> {
    method: Option<Node<'t>>,
    grouped: bool,
    /// O método foi escrito como texto, e não pelo nome da tabela.
    method_text: bool,
    path: Option<Node<'t>>,
    handler: Option<Node<'t>>,
    receiver: Option<Node<'t>>,
    group: Option<Node<'t>>,
    prefix: Option<Node<'t>>,
    scope: Option<Node<'t>>,
    class: Option<Node<'t>>,
    target: Option<Node<'t>>,
    target_module: Option<Node<'t>>,
    module: Option<Node<'t>>,
    global: Option<Node<'t>>,
    exclude: Option<Node<'t>>,
    exclude_method: Option<Node<'t>>,
    exclude_item: Option<Node<'t>>,
    nest: Option<Node<'t>>,
    variable: Option<Node<'t>>,
    value: Option<Node<'t>>,
    parameter: Option<Node<'t>>,
    parameter_name: Option<Node<'t>>,
    parameter_receiver: Option<Node<'t>>,
    call: Option<Node<'t>>,
    argument: Option<Node<'t>>,
}

/// Um prefixo e a quem ele vale: às rotas dentro do trecho `scope`, ou às do
/// nome `target`, escrito na linha `line`. Com `module_path`, o alvo é o
/// caminho de um módulo escrito como texto, que não é nome de nada no
/// arquivo.
struct Prefix {
    value: String,
    scope: Option<(usize, usize)>,
    class: Option<String>,
    target: Option<String>,
    module_path: bool,
    line: usize,
}

/// Um prefixo global como a consulta o dá: o literal e as exclusões, cada uma
/// pelo trecho que a escreve.
#[derive(Default)]
struct GlobalAt {
    value: String,
    items: BTreeMap<(usize, usize), ExclusionAt>,
}

/// Uma exclusão como a consulta a dá: o caminho escrito e o nome do método.
#[derive(Default)]
struct ExclusionAt {
    path: Option<String>,
    method: Option<String>,
}

/// Uma rota antes da junção: se o caminho foi escrito nela, para juntar a de
/// qualquer método com caminho à de método conhecido sem caminho.
struct Built {
    route: Route,
    has_path: bool,
}

/// Até onde se segue um grupo feito de outro grupo; mais fundo que isso é um
/// valor que volta a si mesmo.
const MAX_GROUP_DEPTH: usize = 16;

/// Os grupos que o arquivo monta, para seguir o objeto de uma rota ou de uma
/// chamada até eles.
#[derive(Default)]
struct Groups<'t> {
    nests: Vec<Nest<'t>>,
    bindings: Vec<Binding<'t>>,
    params: Vec<Param>,
}

/// A chamada que faz um grupo: o trecho dela, o prefixo e o objeto de que o
/// grupo nasce.
struct Nest<'t> {
    span: (usize, usize),
    prefix: String,
    object: Option<Node<'t>>,
}

/// A variável que guarda um valor.
struct Binding<'t> {
    name: String,
    value: Node<'t>,
}

/// Um parâmetro que pode levar um grupo: a declaração dele, pela posição em
/// `declarations`, e a posição dele nela ([`OpenPrefix::position`]).
struct Param {
    name: String,
    decl: usize,
    position: Option<usize>,
}

/// Um grupo: os prefixos que ele soma, na ordem, e, quando ele nasce de um
/// parâmetro, a declaração e a posição dele.
#[derive(Default)]
struct Group {
    pieces: Vec<String>,
    open: Option<(usize, Option<usize>)>,
}

impl<'t> Groups<'t> {
    /// Sem chamada que faça grupo nem parâmetro que o leve, nada no arquivo é
    /// grupo.
    fn is_empty(&self) -> bool {
        self.nests.is_empty() && self.params.is_empty()
    }

    /// O grupo que é o valor escrito em `node`. O valor que começa pela
    /// chamada que faz um grupo — a mais de fora delas, quando uma cadeia
    /// tem várias — é esse grupo, somado ao do objeto dela; o nome é o valor
    /// da variável que o guarda, escrita antes na mesma declaração, o
    /// parâmetro da declaração em que está ou, sem nenhum dos dois, a
    /// variável guardada antes no topo do arquivo. `None` para o que não é
    /// grupo.
    fn of(&self, node: Node<'t>, bytes: &[u8], declarations: &[Decl], depth: usize) -> Option<Group> {
        if depth > MAX_GROUP_DEPTH || self.is_empty() {
            return None;
        }
        let (start, end) = (node.start_byte(), node.end_byte());
        let nest = self.nests.iter().filter(|n| n.span.0 == start && n.span.1 <= end).max_by_key(|n| n.span.1);
        if let Some(nest) = nest {
            let mut group =
                nest.object.and_then(|object| self.of(object, bytes, declarations, depth + 1)).unwrap_or_default();
            group.pieces.push(nest.prefix.clone());
            return Some(group);
        }
        let name = node.utf8_text(bytes).ok().filter(|text| is_name(text))?;
        let decl = enclosing(declarations, node.start_position().row + 1);
        let held = |b: &Binding| enclosing(declarations, b.value.start_position().row + 1);
        let before = self.bindings.iter().filter(|b| b.name == name && b.value.start_byte() < start);
        if let Some(binding) = before.clone().filter(|b| held(b) == decl).max_by_key(|b| b.value.start_byte()) {
            return self.of(binding.value, bytes, declarations, depth + 1);
        }
        if let Some(param) = self.params.iter().find(|p| p.name == name && Some(p.decl) == decl) {
            return Some(Group { pieces: Vec::new(), open: Some((param.decl, param.position)) });
        }
        // A variável do topo do arquivo — fora de toda declaração, ou que é
        // ela mesma a declaração que a guarda — vale também dentro das
        // declarações escritas depois dela: a rota da função decorada, cujo
        // decorador é da declaração da função, vê o grupo guardado no topo.
        let top = |b: &&Binding| held(b).is_none_or(|at| declarations[at].name == b.name);
        let binding = before.filter(top).max_by_key(|b| b.value.start_byte())?;
        self.of(binding.value, bytes, declarations, depth + 1)
    }
}

impl RouteRule {
    /// A consulta da regra, compilada na primeira vez que se pede. O padrão
    /// que não compila cai sozinho, com o aviso de sempre, dado então.
    fn compiled(&self) -> Option<&Compiled> {
        self.compiled
            .get_or_init(|| {
                let query = compile_good_query(&self.language, self.raw.query, &self.name())?;
                let roles: Vec<Role> = query.capture_names().iter().map(|name| role(name)).collect();
                let client = roles.iter().any(|role| role.is_client());
                Some(Compiled { query, roles, client })
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

    /// O arquivo liga o framework: a regra é global, ele ou um import global
    /// que o alcança o importa, ou o manifesto mais próximo acima dele o
    /// declara.
    fn active(&self, imports: &[String], project: &Project) -> bool {
        self.raw.global
            || self.imported(imports)
            || self.imported(project.global_imports)
            || self.raw.manifest_dependencies.iter().any(|dep| project.manifest_deps.iter().any(|has| has == dep))
    }

    /// A regra pode achar alguma coisa no texto `bytes`. A global, que liga
    /// em todo arquivo das línguas dela, só roda no que escreve um dos nomes
    /// da tabela `methods`: sem ele, não há chamada a achar, e a consulta nem
    /// se compila.
    fn may_find(&self, bytes: &[u8]) -> bool {
        !self.raw.global
            || self.raw.methods.iter().any(|(name, _)| bytes.windows(name.len()).any(|window| window == name.as_bytes()))
    }

    /// Um dos `imports` é um nome da regra, ou começa por ele seguido de
    /// separador. Letra, algarismo, `_` ou `-` logo depois fazem outro nome,
    /// que não liga a regra.
    fn imported(&self, imports: &[String]) -> bool {
        imports.iter().any(|import| self.names(import))
    }

    /// O import é um nome da regra, ou começa por ele seguido de separador.
    fn names(&self, import: &str) -> bool {
        self.raw.imports.iter().any(|name| {
            import.strip_prefix(name).is_some_and(|rest| {
                rest.chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-'))
            })
        })
    }

    fn method(&self, written: &str) -> Option<&'static str> {
        self.raw.methods.iter().find(|(name, _)| *name == written).map(|(_, method)| *method)
    }

    /// O método HTTP da rota de um match: o do nome escrito, pela tabela
    /// `methods`; o do texto escrito (`"post"`), sem as aspas e em
    /// maiúsculas; sem método capturado, o padrão da regra. `None` para o
    /// nome fora da tabela e o texto que não é nome de método.
    fn stated_method(&self, here: &Captured, bytes: &[u8]) -> Option<String> {
        let Some(node) = here.method else {
            return (!self.raw.default_method.is_empty()).then(|| self.raw.default_method.to_string());
        };
        let written = node.utf8_text(bytes).unwrap_or_default();
        if here.method_text {
            return Some(literal_value(written)).filter(|m| is_method_name(m)).map(str::to_uppercase);
        }
        self.method(written).map(str::to_string)
    }

    /// O caminho escrito sem as marcas `path_trims` das pontas.
    fn trimmed<'p>(&self, path: &'p str) -> &'p str {
        let mut rest = path;
        while let Some(mark) = self.raw.path_trims.iter().find(|mark| rest.starts_with(**mark) || rest.ends_with(**mark)) {
            rest = rest.strip_prefix(mark).unwrap_or(rest);
            rest = rest.strip_suffix(mark).unwrap_or(rest);
        }
        rest
    }

    /// O método HTTP do nome, sem olhar maiúsculas e minúsculas: o nome da
    /// exclusão (`RequestMethod.GET`) não se escreve como o do decorador.
    fn method_ignoring_case(&self, written: &str) -> Option<&'static str> {
        self.raw.methods.iter().find(|(name, _)| name.eq_ignore_ascii_case(written)).map(|(_, method)| *method)
    }

    #[allow(clippy::too_many_lines)] // um passo só: ler os matches e montar cada rota
    fn routes(
        &self,
        compiled: &Compiled,
        root: Node,
        bytes: &[u8],
        declarations: &[Decl],
        skip: &BTreeSet<(usize, usize)>,
    ) -> (Vec<Route>, RouteLinks) {
        let text = |node: Node| node.utf8_text(bytes).unwrap_or_default().to_string();
        let literal = |node: Node| self.trimmed(literal_value(&text(node))).to_string();
        let span = |node: Node| (node.start_byte(), node.end_byte());
        let skipped = |node: Node| {
            let byte = node.start_byte();
            skip.iter().any(|&(start, end)| (start..end).contains(&byte))
        };
        let line_of = |node: Node| node.start_position().row + 1;

        let mut found: Vec<Captured> = Vec::new();
        let mut groups: Vec<((usize, usize), String)> = Vec::new();
        let mut prefixes: Vec<Prefix> = Vec::new();
        let mut file_groups = Groups::default();
        let mut parameters: Vec<(Node, String)> = Vec::new();
        let mut receivers: HashSet<(usize, usize)> = HashSet::new();
        let mut calls: Vec<Captured> = Vec::new();
        let mut whole_modules: HashSet<String> = HashSet::new();
        let mut globals: BTreeMap<(usize, usize), GlobalAt> = BTreeMap::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&compiled.query, root, bytes);
        while let Some(m) = matches.next() {
            let mut here = Captured::default();
            for cap in m.captures {
                let node = Some(cap.node);
                match compiled.roles[cap.index as usize] {
                    Role::Method => here.method = node,
                    Role::GroupedMethod => (here.method, here.grouped) = (node, true),
                    Role::MethodText => (here.method, here.method_text) = (node, true),
                    Role::Path => here.path = node,
                    Role::Handler => here.handler = node,
                    Role::Receiver => here.receiver = node,
                    Role::Group => here.group = node,
                    Role::Prefix => here.prefix = node,
                    Role::Scope => here.scope = node,
                    Role::Class => here.class = node,
                    Role::Target => here.target = node,
                    Role::TargetModule => here.target_module = node,
                    Role::Module => here.module = node,
                    Role::GlobalPrefix => here.global = node,
                    Role::Exclude => here.exclude = node,
                    Role::ExcludeMethod => here.exclude_method = node,
                    Role::ExcludeItem => here.exclude_item = node,
                    Role::Nest => here.nest = node,
                    Role::Variable => here.variable = node,
                    Role::Value => here.value = node,
                    Role::Parameter => here.parameter = node,
                    Role::ParameterName => here.parameter_name = node,
                    Role::ParameterReceiver => here.parameter_receiver = node,
                    Role::Call => here.call = node,
                    Role::Argument => here.argument = node,
                    // Uma regra usa as capturas de um lado só; as da tela
                    // não são do servidor.
                    _ => {}
                }
            }
            if here.method.is_some() {
                found.push(here);
            } else if let (Some(group), Some(path)) = (here.group, here.path) {
                groups.push((span(group), literal(path)));
            } else if here.path.is_some() && !self.raw.default_method.is_empty() {
                found.push(here);
            } else if let (Some(nest), Some(prefix)) = (here.nest, here.prefix) {
                file_groups.nests.push(Nest { span: span(nest), prefix: literal(prefix), object: here.receiver });
            } else if let Some(global) = here.global.filter(|node| !skipped(*node)) {
                let at = globals.entry(span(global)).or_insert_with(|| GlobalAt { value: literal(global), ..GlobalAt::default() });
                if let Some(item) = here.exclude_item.or(here.exclude) {
                    let item = at.items.entry(span(item)).or_default();
                    item.path = here.exclude.map(literal).or(item.path.take());
                    item.method = here.exclude_method.map(text).or(item.method.take());
                }
            } else if let Some(prefix) = here.prefix {
                let target = here.target.or(here.target_module);
                if target.is_some_and(skipped) {
                    continue;
                }
                let module_path = here.target_module.map(|node| literal_value(&text(node)).to_string());
                prefixes.push(Prefix {
                    value: literal(prefix),
                    scope: here.scope.map(span),
                    class: here.class.map(text),
                    module_path: module_path.is_some(),
                    target: here.target.map(text).or(module_path),
                    line: target.map_or(0, line_of),
                });
            } else if let (Some(name), Some(value)) = (here.variable, here.value) {
                file_groups.bindings.push(Binding { name: text(name), value });
            } else if let (Some(parameter), Some(name)) = (here.parameter, here.parameter_name) {
                parameters.push((parameter, text(name)));
            } else if let Some(parameter) = here.parameter_receiver {
                receivers.insert(span(parameter));
            } else if here.call.is_some() {
                calls.push(here);
            } else if let Some(name) = here.module {
                whole_modules.insert(text(name));
            }
        }
        for (node, name) in parameters {
            let Some(decl) = enclosing(declarations, line_of(node)) else { continue };
            let position = (!receivers.contains(&span(node)))
                .then(|| earlier_siblings(node).into_iter().filter(|s| !receivers.contains(&span(*s))).count());
            file_groups.params.push(Param { name, decl, position });
        }
        let open_of = |(decl, position): (usize, Option<usize>)| OpenPrefix {
            declaration: declarations[decl].name.clone(),
            position,
        };

        // O caminho escrito cujo método algum match diz não ganha também a
        // rota do método padrão, do match que o captura sem método.
        let stated: HashSet<(usize, usize)> = found
            .iter()
            .filter(|here| here.method.is_some() && self.stated_method(here, bytes).is_some())
            .filter_map(|here| here.path.map(span))
            .collect();
        let mut built: Vec<Built> = Vec::new();
        let mut reached: HashSet<&str> = HashSet::new();
        for here in found {
            let Some(at) = here.method.or(here.path) else { continue };
            if skipped(at) || (here.method.is_none() && here.path.is_some_and(|path| stated.contains(&span(path)))) {
                continue;
            }
            let byte = at.start_byte();
            let Some(method) = self.stated_method(&here, bytes) else { continue };
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
            let mounted: Vec<&Prefix> = prefixes
                .iter()
                .filter(|p| !p.module_path)
                .filter(|p| {
                    p.target.as_deref().is_some_and(|target| Some(target) == receiver.as_deref() || Some(target) == owner)
                })
                .collect();
            let mut scoped: Vec<&Prefix> = prefixes.iter().filter(|p| p.scope.is_some_and(inside)).collect();
            scoped.sort_by_key(|p| p.scope.map(|(start, end)| (start, std::cmp::Reverse(end))));
            let class = scoped.iter().rev().find_map(|p| p.class.as_deref());

            let path = own.clone().unwrap_or_default();
            if !self.raw.path_starts.is_empty() && !self.raw.path_starts.iter().any(|start| path.starts_with(start)) {
                continue;
            }
            reached.extend(mounted.iter().filter_map(|p| p.target.as_deref()));
            let group = here.receiver.and_then(|node| file_groups.of(node, bytes, declarations, 0));
            let reset = self.raw.reset_marks.iter().find(|mark| path.starts_with(**mark));
            let mut tail: Vec<&str> = group.iter().flat_map(|g| g.pieces.iter().map(String::as_str)).collect();
            if reset.is_none() {
                tail.extend(scoped.iter().map(|p| p.value.as_str()));
            }
            tail.push(reset.map_or(path.as_str(), |mark| &path[mark.len()..]));
            let (handler, handler_line) = handler_of(here.handler, bytes, declarations, line);
            // Cada montagem do objeto ou da declaração da rota é uma rota: a
            // mesma registrada sob dois prefixos atende nos dois.
            let mounts: Vec<Option<&str>> =
                if mounted.is_empty() { vec![None] } else { mounted.iter().map(|p| Some(p.value.as_str())).collect() };
            for mount in mounts {
                let pieces: Vec<&str> = mount.into_iter().chain(tail.iter().copied()).collect();
                let written = joined_path(&pieces);
                built.push(Built {
                    route: Route {
                        method: method.clone(),
                        path: self.normalized(&written, class),
                        written,
                        handler: handler.clone(),
                        line: handler_line,
                        framework: self.raw.framework.to_string(),
                        receiver: receiver.clone().filter(|r| is_name(r)).unwrap_or_default(),
                        owner: owner.unwrap_or_default().to_string(),
                        open: group.as_ref().and_then(|g| g.open).map(open_of),
                        local: None,
                        called_by: Vec::new(),
                    },
                    has_path: own.as_deref().is_some_and(|path| !path.is_empty()),
                });
            }
        }

        let framework = self.raw.framework.to_string();
        let mut links = RouteLinks::default();
        for p in &prefixes {
            let Some(target) = p.target.as_deref().filter(|target| p.module_path || !reached.contains(target)) else {
                continue;
            };
            links.mounts.push(Mount {
                framework: framework.clone(),
                target: target.to_string(),
                line: p.line,
                whole: p.module_path || whole_modules.contains(target),
                module_path: p.module_path,
                prefix: self.prefix_path(&[&p.value]),
            });
        }
        for global in globals.into_values() {
            let excludes = global
                .items
                .into_values()
                .filter_map(|item| Some(self.exclusion(&item.path?, item.method.as_deref())))
                .collect();
            links.globals.push(GlobalPrefix { framework: framework.clone(), prefix: self.prefix_path(&[&global.value]), excludes });
        }
        if !file_groups.is_empty() {
            for call in &calls {
                let Some(name) = call.call.filter(|node| !skipped(*node)).map(text) else { continue };
                if self.method(&name).is_some() {
                    continue;
                }
                let given = call
                    .receiver
                    .map(|object| (None, object))
                    .into_iter()
                    .chain(call.argument.map(|argument| (Some(earlier_siblings(argument).len()), argument)));
                for (position, node) in given {
                    let Some(group) = file_groups.of(node, bytes, declarations, 0) else { continue };
                    let pieces: Vec<&str> = group.pieces.iter().map(String::as_str).collect();
                    links.handoffs.push(Handoff {
                        framework: framework.clone(),
                        name: name.clone(),
                        position,
                        prefix: self.prefix_path(&pieces),
                        from: group.open.map(open_of),
                    });
                }
            }
        }
        (joined(built), links)
    }

    /// As chamadas da tela escritas no arquivo, fora as de um trecho de
    /// teste, e os clientes que ele faz. A chamada cujo objeto é trazido de
    /// outro arquivo guarda o nome dele, e a ligação acha lá a base; a que
    /// é feita por um cliente do arquivo já sai com a base dele.
    fn calls(&self, compiled: &Compiled, root: Node, bytes: &[u8], source: &Source) -> (Vec<RouteCall>, Vec<Client>) {
        let text = |node: Node| node.utf8_text(bytes).unwrap_or_default().to_string();
        let literal = |node: Node| literal_value(&text(node)).to_string();
        let span = |node: Node| (node.start_byte(), node.end_byte());
        let skipped = |node: Node| {
            let byte = node.start_byte();
            source.skip.iter().any(|&(start, end)| (start..end).contains(&byte))
        };
        // O objeto da biblioteca é o nome que o import dela traz; os nomes
        // trazidos pelos outros imports podem ser clientes feitos noutro
        // arquivo.
        let names_of = |library: bool| -> HashSet<String> {
            source
                .brought
                .iter()
                .filter(|(import, _)| self.names(import) == library)
                .flat_map(|(_, names)| names.iter().cloned())
                .collect()
        };
        let (library, elsewhere) = (names_of(true), names_of(false));

        let mut made: BTreeMap<(usize, usize), (String, Option<String>)> = BTreeMap::new();
        let mut written: BTreeMap<(usize, usize), CallAt> = BTreeMap::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&compiled.query, root, bytes);
        while let Some(m) = matches.next() {
            let mut here = CallCaptured::default();
            for cap in m.captures {
                let node = Some(cap.node);
                match compiled.roles[cap.index as usize] {
                    Role::ClientMethod => here.method = node,
                    Role::ClientPath => here.path = node,
                    Role::ClientOption => here.option = node,
                    Role::ClientReceiver => here.receiver = node,
                    Role::ClientMade => here.made = node,
                    Role::ClientFactory => here.factory = node,
                    Role::ClientInstance => here.instance = node,
                    Role::ClientBase => here.base = node,
                    _ => {}
                }
            }
            if let Some(at) = here.made {
                if skipped(at) || !here.factory.is_some_and(|factory| library.contains(&text(factory))) {
                    continue;
                }
                let entry = made.entry(span(at)).or_insert_with(|| (here.instance.map(text).unwrap_or_default(), None));
                entry.1 = here.base.map(literal).or(entry.1.take());
            } else if let (Some(method), Some(path)) = (here.method, here.path) {
                if skipped(method) {
                    continue;
                }
                let entry = written.entry(span(method)).or_insert(CallAt { method, path, receiver: here.receiver, option: None });
                entry.option = here.option.or(entry.option);
            }
        }

        let framework = self.raw.framework.to_string();
        let clients: Vec<Client> = made
            .into_values()
            .map(|(name, base)| Client { framework: framework.clone(), name, base: self.base_path(base.as_deref()) })
            .collect();
        let mut calls = Vec::new();
        for call in written.into_values() {
            let Some(named) = self.method(&text(call.method)) else { continue };
            let method = call.option.map(literal).filter(|m| is_method_name(m)).map_or_else(|| named.to_string(), |m| m.to_uppercase());
            let path = literal(call.path);
            if self.raw.param_wrappers.iter().any(|(open, _)| path.starts_with(*open))
                || self.raw.param_prefixes.iter().any(|prefix| path.starts_with(prefix))
            {
                continue;
            }
            let (base, via) = match call.receiver.map(text) {
                None => (None, String::new()),
                Some(receiver) if library.contains(&receiver) => (None, String::new()),
                Some(receiver) => match clients.iter().find(|c| !c.name.is_empty() && c.name == receiver) {
                    Some(client) => ((!client.base.path.is_empty()).then(|| client.base.clone()), String::new()),
                    None if elsewhere.contains(&receiver) => (None, receiver),
                    None => continue,
                },
            };
            let line = call.method.start_position().row + 1;
            calls.push(RouteCall {
                method,
                path: self.normalized(without_query(&path), None),
                written: path,
                line,
                owner: enclosing(source.declarations, line).map(|i| source.declarations[i].name.clone()).unwrap_or_default(),
                framework: framework.clone(),
                base,
                via,
            });
        }
        (calls, clients)
    }

    /// A base de um cliente, sem o esquema e a máquina do endereço
    /// (`http://localhost:3000`), que não fazem parte do caminho da rota.
    fn base_path(&self, written: Option<&str>) -> RoutePath {
        let Some(written) = written else { return RoutePath::default() };
        let path = written.split_once("://").map_or(written, |(_, rest)| rest.find('/').map_or("", |at| &rest[at..]));
        RoutePath { written: written.to_string(), path: self.normalized(without_query(path), None) }
    }

    /// Um prefixo, escrito com os pedaços juntados por barra e padronizado.
    fn prefix_path(&self, pieces: &[&str]) -> RoutePath {
        let written = joined_path(pieces);
        RoutePath { path: self.normalized(&written, None), written }
    }

    /// A exclusão escrita `written`, do método escrito `method`: o caminho
    /// padronizado como o da rota; o que termina numa marca de
    /// `exclude_wildcards` vale para toda rota que começa pelo resto, com a
    /// barra do fim, quando o resto a tem.
    fn exclusion(&self, written: &str, method: Option<&str>) -> Exclusion {
        let wildcard = self.raw.exclude_wildcards.iter().find(|mark| written.ends_with(**mark));
        let (rest, starts) = wildcard.map_or((written, false), |mark| (&written[..written.len() - mark.len()], true));
        let mut path = self.normalized(rest, None);
        if starts && rest.ends_with('/') && !path.is_empty() {
            path.push('/');
        }
        let method = method.map_or_else(
            || ANY_METHOD.to_string(),
            |name| self.method_ignoring_case(name).map_or_else(|| name.to_uppercase(), str::to_string),
        );
        Exclusion { path, starts, method }
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
    /// parâmetro, e o número ou o id escrito no lugar dele, é todo `{}`; cada
    /// trecho entre as marcas de um parâmetro vira `{}`; o resto fica em
    /// minúsculas.
    fn piece(&self, piece: &str) -> String {
        if self.raw.param_prefixes.iter().any(|prefix| piece.starts_with(prefix)) || is_id(piece) {
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

/// O menor id de letras e algarismos que um pedaço de caminho escreve no
/// lugar de um parâmetro: o do Mongo tem 24, e o UUID, 32 fora os traços.
/// Com menos, o pedaço pode ser uma palavra do caminho.
const MIN_ID_LEN: usize = 16;

/// O pedaço é o valor de um parâmetro escrito no caminho: um número, ou um id
/// de algarismos e das letras de `a` a `f`, com traços ou sem, de
/// [`MIN_ID_LEN`] ou mais, com algum algarismo.
fn is_id(piece: &str) -> bool {
    if !piece.is_empty() && piece.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    let hex = piece.chars().filter(|c| *c != '-');
    hex.clone().count() >= MIN_ID_LEN
        && hex.clone().all(|c| c.is_ascii_hexdigit())
        && piece.chars().any(|c| c.is_ascii_digit())
}

/// O caminho sem a busca e sem a âncora do endereço (`?status=1`, `#fim`).
fn without_query(path: &str) -> &str {
    path.split(['?', '#']).next().unwrap_or_default()
}

/// O texto é o nome de um método HTTP: só letras.
fn is_method_name(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphabetic())
}

/// O caminho padronizado sem os pedaços que dizem a versão da API (`v1`,
/// `v2`): a chamada e a rota que só casam assim casam por suspeita.
pub(crate) fn without_version(path: &str) -> String {
    path.split('/')
        .filter(|piece| !(piece.len() > 1 && piece.starts_with('v') && piece[1..].bytes().all(|b| b.is_ascii_digit())))
        .collect::<Vec<_>>()
        .join("/")
}

/// Uma chamada da tela como a consulta a dá, pela captura do nome chamado.
struct CallAt<'t> {
    method: Node<'t>,
    path: Node<'t>,
    receiver: Option<Node<'t>>,
    option: Option<Node<'t>>,
}

/// As capturas da tela de um match, pelo papel.
#[derive(Default)]
struct CallCaptured<'t> {
    method: Option<Node<'t>>,
    path: Option<Node<'t>>,
    option: Option<Node<'t>>,
    receiver: Option<Node<'t>>,
    made: Option<Node<'t>>,
    factory: Option<Node<'t>>,
    instance: Option<Node<'t>>,
    base: Option<Node<'t>>,
}

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

/// Os pedaços de um caminho juntados por barra, sem as barras das pontas de
/// cada um e sem os vazios.
fn joined_path(pieces: &[&str]) -> String {
    pieces.iter().map(|piece| piece.trim_matches('/')).filter(|piece| !piece.is_empty()).collect::<Vec<_>>().join("/")
}

/// O prefixo `outer` na frente do caminho `inner`, no escrito e no
/// padronizado.
fn joined_prefix(outer: &RoutePath, inner: &RoutePath) -> RoutePath {
    RoutePath {
        written: joined_path(&[&outer.written, &inner.written]),
        path: joined_path(&[&outer.path, &inner.path]),
    }
}

/// O texto é um nome só: letras, algarismos, `_` e `$`, sem começar por
/// algarismo.
fn is_name(text: &str) -> bool {
    text.chars().next().is_some_and(|c| !c.is_ascii_digit())
        && text.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Os irmãos do nó, do mesmo tipo dele, escritos antes dele: a posição dele
/// entre os parâmetros ou os argumentos.
fn earlier_siblings(node: Node) -> Vec<Node> {
    let mut out = Vec::new();
    let mut at = node.prev_named_sibling();
    while let Some(sibling) = at {
        if sibling.kind_id() == node.kind_id() {
            out.push(sibling);
        }
        at = sibling.prev_named_sibling();
    }
    out
}

/// Os nomes trazidos de outro arquivo que as rotas e as chamadas pedem, com
/// a posição do arquivo em `modules`: os montados que cada arquivo não
/// registra ([`Mount`]), que [`across_files`] segue até as rotas de onde
/// eles vêm, e os objetos das chamadas da tela feitas por um cliente de
/// outro arquivo ([`RouteCall::via`]), que a ligação segue até a base dele.
pub(crate) fn brought_names(modules: &[Module]) -> Vec<(usize, String)> {
    let names: BTreeSet<(usize, String)> = modules
        .iter()
        .enumerate()
        .flat_map(|(at, m)| {
            let mounted = m.route_links.mounts.iter().filter(|mount| !mount.module_path).map(|mount| mount.target.clone());
            let via = m.route_calls.iter().filter(|call| !call.via.is_empty()).map(|call| call.via.clone());
            mounted.chain(via).map(move |name| (at, name))
        })
        .collect();
    names.into_iter().collect()
}

/// Os caminhos de módulo que as montagens escrevem como texto
/// ([`Mount::module_path`]), com a posição do arquivo em `modules`: cada um
/// se lê como import do arquivo que o escreve, e [`across_files`] segue até
/// as rotas do arquivo que ele nomeia.
pub(crate) fn module_paths(modules: &[Module]) -> Vec<(usize, String)> {
    let paths: BTreeSet<(usize, String)> = modules
        .iter()
        .enumerate()
        .flat_map(|(at, m)| {
            m.route_links.mounts.iter().filter(|mount| mount.module_path).map(move |mount| (at, mount.target.clone()))
        })
        .collect();
    paths.into_iter().collect()
}

/// A soma entre arquivos, refeita em toda passada, inteira ou não, a partir
/// do que cada arquivo guarda. Cada rota volta ao caminho que o próprio
/// arquivo escreve ([`Route::local_form`]) e ganha, na frente, os prefixos
/// que outros arquivos escrevem para ela:
///
/// - a montagem de um nome trazido de outro arquivo ([`Mount`]). O arquivo de
///   onde o nome vem é o que o import que o traz nomeia (`brought`, pela
///   posição do arquivo e pelo nome, de [`brought_names`]) ou o da
///   declaração a que liga a chamada escrita na linha da montagem. O do
///   caminho de módulo escrito como texto é o que ele nomeia, lido como
///   import de quem o escreve (`brought`, de [`module_paths`]);
/// - a entrega de um grupo ([`Handoff`]) às rotas em aberto de toda
///   declaração com o nome e a posição dela, no projeto do arquivo que a
///   escreve. A entrega cujo grupo nasce de um parâmetro soma, na frente, o
///   que chega a esse parâmetro, em cadeia, e a cadeia que volta a um lugar
///   por onde passou para ali;
/// - o prefixo global ([`GlobalPrefix`]), por fora de tudo, nas rotas do
///   framework dele no projeto do arquivo que o escreve, fora as que uma
///   exclusão tira.
///
/// Dois prefixos para a mesma rota dão a rota uma vez com cada um. A rota em
/// aberto a que nada chega fica com o caminho do arquivo. A rota que um
/// prefixo de fora mudou guarda o caminho de antes em [`Route::local`]: a
/// passada que não relê o arquivo parte dele, e não soma duas vezes.
pub(crate) fn across_files(
    modules: &mut [Module],
    manifests: &[Manifest],
    brought: &BTreeMap<(usize, String), Vec<String>>,
) {
    for m in modules.iter_mut().filter(|m| m.routes.iter().any(|r| r.local.is_some())) {
        let mut local: Vec<Route> = m.routes.iter().map(Route::local_form).collect();
        local.sort();
        local.dedup();
        m.routes = local;
    }
    if modules.iter().all(|m| m.route_links.is_empty()) {
        return;
    }
    let mut middle: ByRoute<BTreeSet<RoutePath>> = HashMap::new();
    mounted(modules, brought, &mut middle);
    delivered(modules, manifests, &mut middle);
    let (globals, outer) = global(modules, manifests);
    for (at, m) in modules.iter_mut().enumerate() {
        if !(0..m.routes.len()).any(|ri| middle.contains_key(&(at, ri)) || outer.contains_key(&(at, ri))) {
            continue;
        }
        let mut routes = Vec::new();
        for (ri, route) in m.routes.iter().enumerate() {
            let local = RoutePath { written: route.written.clone(), path: route.path.clone() };
            let middles: Vec<RoutePath> =
                middle.get(&(at, ri)).map_or_else(|| vec![RoutePath::default()], |found| found.iter().cloned().collect());
            for mid in middles {
                let inner = joined_prefix(&mid, &local);
                let reaching: Vec<Option<&RoutePath>> = match outer.get(&(at, ri)) {
                    None => vec![None],
                    Some(found) => found
                        .iter()
                        .map(|&g| &globals[g])
                        .map(|g| (!g.excludes.iter().any(|e| e.covers(&route.method, &inner.path))).then_some(&g.prefix))
                        .collect(),
                };
                for global in reaching {
                    let full = global.map_or_else(|| inner.clone(), |g| joined_prefix(g, &inner));
                    let changed = full != local;
                    routes.push(Route {
                        written: full.written,
                        path: full.path,
                        local: changed.then(|| local.clone()),
                        ..route.clone()
                    });
                }
            }
        }
        routes.sort();
        routes.dedup();
        m.routes = routes;
    }
}

/// As montagens de nomes trazidos de outro arquivo: o prefixo de cada uma
/// vai, em `middle`, para as rotas que ela alcança no arquivo de onde o nome
/// vem. O nome que recebe o módulo inteiro, pelo import, alcança todas as
/// rotas do framework no arquivo; o outro, as registradas num objeto com o
/// nome dele ou escritas numa declaração com o nome dele.
fn mounted(
    modules: &[Module],
    brought: &BTreeMap<(usize, String), Vec<String>>,
    middle: &mut ByRoute<BTreeSet<RoutePath>>,
) {
    if modules.iter().all(|m| m.route_links.mounts.is_empty()) {
        return;
    }
    let index: HashMap<&str, usize> = modules.iter().enumerate().map(|(at, m)| (m.path.as_str(), at)).collect();
    for (at, m) in modules.iter().enumerate() {
        for mount in &m.route_links.mounts {
            let imported: BTreeSet<usize> = brought
                .get(&(at, mount.target.clone()))
                .into_iter()
                .flatten()
                .filter_map(|path| index.get(path.as_str()).copied())
                .collect();
            let called: BTreeSet<usize> = modules
                .iter()
                .enumerate()
                .filter(|(_, other)| {
                    other.declarations.iter().any(|d| {
                        d.name == mount.target && d.used_by.iter().any(|u| u.file == m.path && u.line == mount.line)
                    })
                })
                .map(|(to, _)| to)
                .collect();
            for to in imported.union(&called).copied().filter(|&to| to != at) {
                let whole = mount.whole && imported.contains(&to);
                for (ri, r) in modules[to].routes.iter().enumerate() {
                    if r.framework == mount.framework && (whole || r.receiver == mount.target || r.owner == mount.target) {
                        middle.entry((to, ri)).or_default().insert(mount.prefix.clone());
                    }
                }
            }
        }
    }
}

/// Um valor por rota: a chave é a posição do arquivo em `modules` e a da rota
/// entre as rotas dele.
type ByRoute<T> = HashMap<(usize, usize), T>;

/// As entregas de grupo do projeto, pelo lugar a que chegam: o framework, o
/// nome da declaração e a posição.
type Sites<'m> = HashMap<(&'m str, &'m str, Option<usize>), Vec<(usize, &'m Handoff)>>;

/// As entregas de grupo: os prefixos que chegam a cada rota em aberto vão
/// para ela em `middle`. A rota a que só chega o caminho vazio fica como
/// está.
fn delivered(modules: &[Module], manifests: &[Manifest], middle: &mut ByRoute<BTreeSet<RoutePath>>) {
    let mut sites: Sites = HashMap::new();
    for (at, m) in modules.iter().enumerate() {
        for h in &m.route_links.handoffs {
            sites.entry((h.framework.as_str(), h.name.as_str(), h.position)).or_default().push((at, h));
        }
    }
    if sites.is_empty() {
        return;
    }
    let reach = Reach { modules, manifests, sites };
    let mut known: HashMap<(usize, &str, &OpenPrefix), BTreeSet<RoutePath>> = HashMap::new();
    for (at, m) in modules.iter().enumerate() {
        for (ri, r) in m.routes.iter().enumerate() {
            let Some(open) = &r.open else { continue };
            let found = known
                .entry((at, r.framework.as_str(), open))
                .or_insert_with(|| reach.prefixes(at, &r.framework, open, &mut Vec::new()));
            if found.iter().all(|p| p.written.is_empty() && p.path.is_empty()) {
                continue;
            }
            middle.entry((at, ri)).or_default().extend(found.iter().cloned());
        }
    }
}

/// O que se segue para achar o que chega a um parâmetro em aberto.
struct Reach<'m> {
    modules: &'m [Module],
    manifests: &'m [Manifest],
    sites: Sites<'m>,
}

impl Reach<'_> {
    /// Os prefixos que chegam ao parâmetro `open` da declaração vista do
    /// arquivo `at`: os das entregas a ele escritas no projeto que contém o
    /// arquivo, cada uma com o que chega, na frente, ao parâmetro de que o
    /// grupo dela nasce. Só o caminho vazio quando nada chega; nada quando a
    /// cadeia volta a um lugar de `passed`.
    fn prefixes(
        &self,
        at: usize,
        framework: &str,
        open: &OpenPrefix,
        passed: &mut Vec<(usize, OpenPrefix)>,
    ) -> BTreeSet<RoutePath> {
        let path = &self.modules[at].path;
        let incoming: Vec<(usize, &Handoff)> = self
            .sites
            .get(&(framework, open.declaration.as_str(), open.position))
            .into_iter()
            .flatten()
            .filter(|(from, _)| is_under(path, project_dir(&self.modules[*from].path, self.manifests)))
            .copied()
            .collect();
        if incoming.is_empty() {
            return BTreeSet::from([RoutePath::default()]);
        }
        let here = (at, open.clone());
        if passed.contains(&here) {
            return BTreeSet::new();
        }
        passed.push(here);
        let mut out = BTreeSet::new();
        for (from, handoff) in incoming {
            match &handoff.from {
                None => {
                    out.insert(handoff.prefix.clone());
                }
                Some(before) => {
                    for p in self.prefixes(from, framework, before, passed) {
                        out.insert(joined_prefix(&p, &handoff.prefix));
                    }
                }
            }
        }
        passed.pop();
        out
    }
}

/// Os prefixos globais do projeto e, de cada rota que um alcança, pela
/// posição do arquivo e dela, as posições deles na lista: as rotas do
/// framework do prefixo sob a pasta do projeto do arquivo que o escreve.
fn global(modules: &[Module], manifests: &[Manifest]) -> (Vec<GlobalPrefix>, ByRoute<Vec<usize>>) {
    let mut list = Vec::new();
    let mut reach: ByRoute<Vec<usize>> = HashMap::new();
    for m in modules {
        for g in &m.route_links.globals {
            let dir = project_dir(&m.path, manifests);
            for (at, other) in modules.iter().enumerate().filter(|(_, other)| is_under(&other.path, dir)) {
                for (ri, _) in other.routes.iter().enumerate().filter(|(_, r)| r.framework == g.framework) {
                    reach.entry((at, ri)).or_default().push(list.len());
                }
            }
            list.push(g.clone());
        }
    }
    (list, reach)
}
