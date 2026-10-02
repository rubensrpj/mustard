//! The intermediate "project model".
//!
//! Produzido pelas etapas determinísticas da análise e consumido pelas
//! projeções. Nada aqui codifica framework nenhum. Os grupos por sufixo do nome
//! (`roles` e `conventions`) saíram do modelo: nenhum leitor sobrou, e era por
//! eles que as skills fracas nasciam.
//!
//! O modelo se grava no mapa do projeto, o banco que a porta do núcleo
//! declara (`mustard_core::io::project_map`), e se lê dele de volta: o que o
//! banco não guarda — a cobertura além das pastas puladas e se o grafo tem
//! ciclo — só serve ao resumo impresso da passada, e volta vazio.

use mustard_core::domain::normalize::Languages;
use mustard_core::domain::project_map::{History, Quality};
use mustard_core::domain::vocabulary::stacks::StackDetection;
use mustard_core::io::project_map as store;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ProjectModel {
    pub root: String,
    pub languages: Vec<LanguageStat>,
    pub manifests: Vec<Manifest>,
    pub frameworks: Vec<String>,
    pub skeleton: Vec<SkeletonEntry>,
    pub modules: Vec<Module>,
    pub graph: GraphStats,
    /// What the scan visited vs skipped — verifiable answer to "did you read it all?".
    #[serde(default)]
    pub coverage: Coverage,
    /// Projects/compilation units in the workspace, one per build manifest.
    #[serde(default)]
    pub projects: Vec<ProjectUnit>,
    /// Stacks inferred by evidence convergence (manifest deps + path markers +
    /// code signatures). The engine and the registry live in `mustard-core` —
    /// stacks are DATA there, never names in this crate. Additive: older
    /// models without the field keep deserialising.
    #[serde(default)]
    pub detected_stacks: Vec<StackDetection>,
    /// Where this pass read from — the commit and the files not committed.
    #[serde(default)]
    pub state: ScanState,
    /// The git history, one entry per commit (created and changed files).
    /// Sem commit, fica o nome da branch de partida e o motivo da falta.
    #[serde(default, skip_serializing_if = "is_blank_history")]
    pub history: History,
    /// A marca de cada bloco do mapa de onde o modelo foi lido, pelo nome do
    /// bloco: a versão do scan que o encheu, ou vazia no bloco que voltou
    /// vazio. Não se grava como chave: a marca desta passada vai à gravação.
    #[serde(skip)]
    pub marks: BTreeMap<String, String>,
}

impl ProjectModel {
    /// O modelo gravado no mapa em `path`; `None` sem mapa, ou com um que não
    /// se lê — a passada então lê tudo.
    pub fn load(path: &Path) -> Option<Self> {
        let stored = store::read_stored_at(path).ok()?;
        let mut model: Self = serde_json::from_str(&stored.json).ok()?;
        model.marks = stored.marks;
        Some(model)
    }

    /// O estado da leitura gravado no mapa em `path`: o censo, de cada
    /// arquivo só o caminho, o blob e os sinais de código, e as marcas dos
    /// blocos. As declarações, o grafo e a história ficam no banco. `None`
    /// sem mapa, ou com um que não se lê.
    pub fn load_state(path: &Path) -> Option<Self> {
        let stored = store::read_state_at(path).ok()?;
        let mut model: Self = serde_json::from_str(&stored.json).ok()?;
        model.marks = stored.marks;
        Some(model)
    }

    /// O modelo lido do mapa em `path`, com o motivo quando não se lê.
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let stored = store::read_stored_at(path).map_err(|refusal| anyhow::anyhow!("{}: {refusal:?}", path.display()))?;
        Ok(serde_json::from_str(&stored.json)?)
    }

    /// Grava o modelo no mapa em `path`, com a marca `mark` em cada bloco.
    /// Só os blocos que mudaram se regravam, numa transação só; com tudo
    /// igual, nada se grava e a resposta é `false`. O índice de busca que se
    /// refaz com os arquivos e as declarações prepara as palavras nas línguas
    /// `languages`.
    pub fn save(&self, path: &Path, mark: &str, languages: &Languages) -> anyhow::Result<bool> {
        Ok(store::save_at(path, &serde_json::to_value(self)?, mark, languages)?)
    }

    /// Grava só o censo do modelo no mapa em `path`, com a marca `mark`; os
    /// outros blocos ficam como estão. Com o censo igual, nada se grava e a
    /// resposta é `false`.
    pub fn save_census(&self, path: &Path, mark: &str) -> anyhow::Result<bool> {
        Ok(store::save_block_at(path, &store::CENSUS, &serde_json::to_value(self)?, mark)?)
    }
}

/// One compilation unit / project in the workspace (one per build manifest)
/// and how many source files live under it.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ProjectUnit {
    pub name: String,
    pub dir: String,
    pub kind: String,
    pub code_files: usize,
    /// Frameworks/deps that recur across this unit's own manifests — the same
    /// frequency-ranked projection [`crate::ingest`] applies repo-wide, restricted
    /// to the manifests under `dir`. No catalog; agnostic to language/framework.
    #[serde(default)]
    pub frameworks: Vec<String>,
    /// Build/codegen scripts declared by this unit's manifests, verbatim —
    /// aggregated, deduped, sorted.
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Stacks inferred for this unit (same engine/contract as
    /// [`ProjectModel::detected_stacks`]). Additive — defaults to empty so
    /// older payloads keep deserialising; population is per-unit evidence,
    /// owned by the consumer-side projection.
    #[serde(default)]
    pub detected_stacks: Vec<StackDetection>,
}

/// What the scan actually visited — so "did you read everything?" is verifiable.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Coverage {
    pub top_dirs: Vec<DirCoverage>,
    /// As pastas que a caminhada pulou pela lista do `manifests.toml`, pelo
    /// caminho relativo, em qualquer profundidade: as que nunca guardam
    /// código do projeto e as de saída ou de dependências sem arquivo de
    /// código no índice do git. A pasta dentro de outra pulada não aparece.
    pub skipped_build_dirs: Vec<String>,
    /// Extensions seen but not mined (not a supported source language).
    pub unsupported_exts: Vec<ExtCount>,
    pub code_files_read: usize,
    pub non_utf8_skipped: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DirCoverage {
    pub dir: String,
    pub code_files: usize,
    pub other_files: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ExtCount {
    pub ext: String,
    pub count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LanguageStat {
    pub language: String,
    pub files: usize,
    pub loc: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Manifest {
    pub path: String,
    pub kind: String,
    pub dependencies: Vec<String>,
    /// Build/codegen scripts declared by the manifest, verbatim ("name: cmd").
    /// Surfaced as-is (no catalog) so a `generate`/codegen step is identified
    /// from the repo's own scripts, not from a hardcoded list.
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Project name derived per the manifest's rule (stem or parent dir).
    #[serde(default)]
    pub name: String,
    /// The module path the manifest declares for import resolution, when it
    /// declares one — kept so a pass that does not re-read the manifest still
    /// resolves imports the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// The package's own name, as the manifest declares it — kept so imports
    /// that name another package of the same project resolve inside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// The namespace the manifest declares as its project's default — kept so
    /// a pass that does not re-read the manifest still names the pages of the
    /// project the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SkeletonEntry {
    pub dir: String,
    pub role: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Module {
    pub path: String,
    /// O id do blob do git do conteúdo que a passada leu; vazio fora do git.
    /// A passada seguinte relê o arquivo só quando o blob de agora é outro.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub blob: String,
    pub language: String,
    pub loc: usize,
    pub imports: Vec<String>,
    /// The imports this file puts in sight of every file of its language
    /// under the same project (the folder of the nearest manifest above it),
    /// not only of itself. They are in `imports` too: the import edge stays on
    /// this file alone. Written only when there is one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub global_imports: Vec<String>,
    /// Os imports escritos dentro de um trecho de teste do próprio arquivo,
    /// como foram escritos. Não estão em `imports`: não são dependência do
    /// arquivo, e por isso ficam fora de `deps` e do grafo. Written only when
    /// there is one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_imports: Vec<String>,
    /// Os arquivos do projeto que o trecho de teste deste importa, resolvidos
    /// como os de `deps` e guardados à parte: dizem o que o teste cobre. O
    /// próprio arquivo não entra.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_deps: Vec<String>,
    /// As linhas, da primeira à última, de cada trecho de teste do arquivo. A
    /// chamada e a citação escritas nelas são do teste, e não uso das
    /// declarações que nomeiam. Guardadas com o módulo, para que a passada que
    /// não relê o arquivo saiba o mesmo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_lines: Vec<(usize, usize)>,
    /// As linhas, da primeira à última, de cada módulo com corpo escrito
    /// dentro do arquivo, o trecho de teste incluído. O caminho escrito dentro
    /// de N deles que começa pelo `parent_alias` da língua sai primeiro desses
    /// N módulos, e só depois sobe pasta. Guardadas com o módulo, para que a
    /// passada que não relê o arquivo saiba o mesmo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub module_lines: Vec<(usize, usize)>,
    /// As linhas em que o arquivo escreve cada import que ele escreve ao menos
    /// uma vez dentro de um dos seus módulos ([`Module::module_lines`]), todas
    /// elas, dentro e fora. O import que não está aqui só é escrito fora de
    /// qualquer módulo do arquivo.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub import_lines: BTreeMap<String, Vec<usize>>,
    /// Cada caminho do projeto escrito antes do nome numa chamada, o que virou
    /// import (`super::super::x` em `super::super::x::valor()`), com as
    /// chamadas escritas por ele fora do trecho de teste, no mesmo formato de
    /// [`Module::calls`]. A chamada que está aqui liga só às declarações dos
    /// arquivos que o caminho nomeia. Guardados com o módulo, para que a
    /// passada que não relê o arquivo ligue igual.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub call_paths: BTreeMap<String, Vec<CallSite>>,
    /// Cada caminho de duas partes ou mais escrito antes do nome numa
    /// chamada, o que não virou import (`std::fs` em `std::fs::read()`), com
    /// as chamadas escritas por ele fora do trecho de teste. A chamada que
    /// está aqui não liga ao projeto quando a raiz do caminho é de fora dele.
    /// Guardados com o módulo, para que a passada que não relê o arquivo ligue
    /// igual.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub other_call_paths: BTreeMap<String, Vec<CallSite>>,
    /// De cada import do arquivo, como foi escrito, os nomes que ele traz
    /// (`@imported`), cada um com o nome que tem no arquivo de origem
    /// (`{"./pasta": {"L": "Leitor"}}` para `import { Leitor as L } from
    /// './pasta'`); o mesmo nome quando o import não o troca, e também quando
    /// o nome é o módulo inteiro (`s` em `import loja.servico as s`). O
    /// arquivo alvo é pedido pelo nome de origem, e o nome escrito no corpo
    /// liga às declarações pelo nome que elas têm onde a resolução chegou. O
    /// nome que um import de fora do projeto traz é de fora: escrito sozinho
    /// ou antes de outro nome, não liga a nada do projeto. A resolução é do
    /// projeto inteiro e se refaz a cada passada, por isso os nomes ficam com
    /// o módulo, para que a passada que não relê o arquivo ligue igual.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub brought: BTreeMap<String, BTreeMap<String, String>>,
    /// De cada repasse do arquivo, como foi escrito (`export * from './x'`,
    /// `pub use a::B`), os nomes que ele oferece a quem importa o arquivo,
    /// cada um com o nome que tem no arquivo de origem; `*` como nome
    /// oferecido quando oferece todos, e `*` como nome de origem quando o
    /// nome oferecido é o arquivo inteiro que o caminho nomeia (`util` em
    /// `export * as util from './util'`). O nome que o arquivo não declara,
    /// mas repassa, liga quem o importa ao arquivo que o declara. Fica com o
    /// módulo pelo mesmo motivo dos `brought`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reexports: BTreeMap<String, BTreeMap<String, String>>,
    /// Os nomes que abrem a cadeia escrita antes de uma chamada — o
    /// qualificador sem nada antes dele (`File` em `File.ReadAllText()`) ou a
    /// primeira parte do caminho dela (`System` em
    /// `System.IO.File.ReadAllText()`) — e que o arquivo não liga: nenhum é
    /// nome local (`@local`) nem o próprio objeto (`self_receivers`,
    /// `parent_receivers`). O grafo toma a chamada aberta por um deles que
    /// não é peça do projeto como chamada de biblioteca. Ficam com o módulo
    /// pelo mesmo motivo dos `brought`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unbound_heads: Vec<String>,
    pub namespaces: Vec<String>,
    pub declarations: Vec<Decl>,
    /// Machine-written class, when one applies: "generated" | "vendored" |
    /// "lockfile" | "minified" (empty = hand-written). Decided by the generic
    /// engine in `classify` from catalog DATA (generated-markers.toml) plus
    /// the repo's own overrides (.gitattributes / .editorconfig). Additive:
    /// older models keep deserialising; hand-written modules don't serialise it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file_class: String,
    /// Which marker decided `file_class` (catalog literal/regex/glob or the
    /// override attribute) — provenance, so a classification is explainable.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub marker: String,
    /// The project files this one imports, resolved through the graph — the
    /// reverse of "who imports this file". A importação de um namespace conta
    /// só para os arquivos dele que declaram um nome que este chama ou cita;
    /// o namespace importado sem nenhum nome usado não conta para nenhum.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
    /// The test files that cover this one: a test that imports it, or a test
    /// that keeps changing together with it in git.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<String>,
    /// The file carries its own tests (an inline test marker).
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_tests: bool,
    /// As medidas de qualidade do arquivo, refeitas do projeto inteiro a cada
    /// passada ([`crate::quality`]). Written only when measured.
    #[serde(default, skip_serializing_if = "Quality::is_empty")]
    pub quality: Quality,
    /// The stack code signatures found in this file's content, kept so a pass
    /// that does not read the file again still infers the same stacks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<String>,
    /// Every call site read out of this file: the name called and the line it
    /// is called on, in document order. Raw on purpose — a name is not
    /// resolved to a declaration here, so a file that did not change still
    /// feeds the declaration links of a pass that only read what changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<CallSite>,
    /// Every name cited without being called that names a constant or a
    /// type some file in sight of this one declares, with the line it is
    /// cited on: outside comments, quoted text, decorations, imports, the
    /// names an import brings in, namespace names and its own declaration
    /// header. The letter the name starts with decides nothing; a name no
    /// file in sight declares is not kept. Not resolved to a
    /// declaration, for the same reason as [`Module::calls`], and written the
    /// same way.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cites: Vec<CallSite>,
    /// Every name written where a value goes (an argument, the right side of
    /// an assignment, as the language's query marks it) without being called
    /// there, with the line: the function handed to another one or kept in a
    /// name. Raw, like [`Module::calls`]: it links only to a function or a
    /// method in sight of the file, and a pass that does not read the file
    /// again links it the same way.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_uses: Vec<CallSite>,
    /// Cada nome de membro escrito depois do objeto que o tem, sem chamada
    /// ali (`pedido.Total`, `self.total`), com a linha e o nome escrito antes
    /// dele: a propriedade ou o campo lido ou escrito. Cru, como
    /// [`Module::calls`]: liga só a uma propriedade ou a um campo que o
    /// objeto alcança, do jeito que liga a chamada de método escrita depois
    /// do mesmo objeto, e a passada que não relê o arquivo liga do mesmo
    /// jeito.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub member_reads: Vec<CallSite>,
    /// Os textos fixos do arquivo (`@text`), fora os do trecho de teste, em
    /// ordem de linha. O arquivo de teste e o escrito por máquina não guardam
    /// nenhum. Guardados com o módulo, para que a passada que não relê o
    /// arquivo guarde os mesmos. Written only when there is one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<Text>,
    /// As rotas do servidor registradas no arquivo ([`Route`]), achadas pela
    /// regra do framework que ele importa, fora as do trecho de teste. O
    /// arquivo de teste e o escrito por máquina não guardam nenhuma.
    /// Guardadas com o módulo, como os textos fixos. Written only when there
    /// is one. A rota que um prefixo escrito noutro arquivo alcança já sai com
    /// ele, e guarda o caminho só com o que o próprio arquivo escreve
    /// ([`Route::local`]): cada passada soma de novo, a partir dele, os
    /// prefixos de fora.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<Route>,
    /// Os prefixos que o arquivo escreve para rotas de outros arquivos
    /// ([`RouteLinks`]). Guardados com o módulo, como as rotas, para que a
    /// passada que não relê o arquivo some os mesmos. Written only when there
    /// is one.
    #[serde(default, skip_serializing_if = "RouteLinks::is_empty")]
    pub route_links: RouteLinks,
    /// As chamadas da tela a rotas do servidor escritas no arquivo
    /// ([`RouteCall`]), achadas pela regra do cliente que ele liga, fora as
    /// do trecho de teste. Guardadas com o módulo, como as rotas: a ligação
    /// de cada uma à rota que ela alcança se refaz em toda passada. Written
    /// only when there is one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub route_calls: Vec<RouteCall>,
    /// Os comentários do começo do arquivo, antes do primeiro código — a
    /// documentação do módulo, o cabeçalho do arquivo —, limpos das marcas e
    /// juntados numa linha. O arquivo escrito por máquina não guarda. Written
    /// only when there is one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file_doc: String,
    /// Os outros comentários do arquivo, fora os do começo, que caem fora das
    /// linhas de toda declaração, limpos das marcas e juntados numa linha, na
    /// ordem em que foram escritos: os de dentro de uma declaração estão no
    /// [`Decl::body_comment`] dela, e o índice de busca junta os dois, cada
    /// comentário uma vez. O arquivo escrito por máquina não guarda. Written
    /// only when there is one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file_comment: String,
    /// Quantos bytes do começo do [`Decl::body_comment`] da primeira
    /// declaração de fora (`outer_declarations`, no núcleo) são comentários
    /// do começo do arquivo, os de [`Module::file_doc`]: o comentário escrito
    /// antes de todo código na primeira linha dela é das duas, e o índice o
    /// pula ao juntar os comentários do arquivo. Written only when it is not
    /// zero.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub file_doc_in_body: usize,
}

/// Um texto fixo escrito no código: a linha, a marca — [`TEXT_LOG`],
/// [`TEXT_ERROR`] ou [`TEXT_PLAIN`] —, o valor sem as aspas, numa linha, e o
/// nome da declaração que contém a linha (vazio fora de toda declaração).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Text {
    pub line: usize,
    pub kind: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
}

/// A marca do texto escrito numa chamada de log (`log_calls` em
/// languages.toml).
pub const TEXT_LOG: &str = "log";

/// A marca do texto escrito no que lança ou devolve erro (`error_forms` em
/// languages.toml).
pub const TEXT_ERROR: &str = "error";

/// A marca de todo outro texto fixo.
pub const TEXT_PLAIN: &str = "text";

/// Uma rota do servidor: o método HTTP ([`ANY_METHOD`] quando vale qualquer
/// um), o caminho padronizado — em minúsculas, sem barra no começo nem no
/// fim, cada parâmetro como `{}` —, o caminho como foi escrito, com os
/// prefixos juntados por barra, quem a atende — o nome da função, vazio
/// quando ela é escrita ali mesmo, e a linha dela no arquivo — e o framework
/// cuja regra a achou. O método e o caminho padronizado são a chave da rota.
///
/// O resto diz como os prefixos escritos noutros arquivos a alcançam: o nome
/// do objeto em que ela se registra, o da declaração que a contém, o prefixo
/// em aberto, os lugares da montagem de fora por que ela passa e, quando
/// algum prefixo de fora a mudou, o caminho só com o que o próprio arquivo
/// escreve.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub written: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub handler: String,
    pub line: usize,
    pub framework: String,
    /// O nome do objeto em que a rota se registra (`router` em
    /// `router.get(…)`); vazio quando ele não é um nome só.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub receiver: String,
    /// O nome da declaração mais interna que contém a rota; vazio fora de
    /// toda declaração.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
    /// O prefixo em aberto: o objeto da rota, seguidos os grupos do arquivo, é
    /// um parâmetro da declaração que a contém. Quem chama a declaração com um
    /// grupo nessa posição lhe dá o prefixo ([`Handoff`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<OpenPrefix>,
    /// Os lugares da montagem mais de fora do arquivo por que a rota passa —
    /// o objeto que a recebe e a declaração em que está escrita —, quando o
    /// arquivo não monta nada nesse lugar e a montagem nasce de um nome, que
    /// outro arquivo pode montar. A montagem de outro arquivo num desses
    /// lugares põe o prefixo dela na frente do caminho da rota, como na rota
    /// registrada no objeto com o nome dela.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub through: Vec<String>,
    /// O caminho só com o que o próprio arquivo escreve, quando um prefixo de
    /// outro arquivo o mudou; `None` quando nenhum mudou.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<RoutePath>,
    /// As chamadas da tela que alcançam a rota ([`RouteCall`]), como o uso
    /// de uma declaração: o arquivo e a linha da chamada e a declaração de
    /// onde ela parte. Provada quando o método e o caminho casam exato e com
    /// uma rota só; suspeita, com as funções que atendem cada rota que ela
    /// pode alcançar, quando casa com mais de uma ou só casa sem o prefixo de
    /// versão ou sem a base do cliente. Refeitas em toda passada.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub called_by: Vec<UseSite>,
}

impl Route {
    /// A rota só com o que o próprio arquivo escreve: sem os prefixos de
    /// fora que uma passada lhe somou nem as chamadas da tela que a
    /// alcançam.
    #[must_use]
    pub fn local_form(&self) -> Self {
        let route = Self { called_by: Vec::new(), ..self.clone() };
        match &self.local {
            Some(local) => Self { written: local.written.clone(), path: local.path.clone(), local: None, ..route },
            None => route,
        }
    }
}

/// Uma chamada da tela a uma rota do servidor: o método HTTP, o caminho
/// padronizado como o da rota, sem a base do cliente, o caminho como foi
/// escrito, a linha da chamada, o nome da declaração mais interna que a
/// contém (vazio fora de toda declaração) e o framework cuja regra a achou.
///
/// A base do cliente — o endereço que o objeto que faz a chamada põe na
/// frente de todo caminho — vem escrita no próprio arquivo em `base`, ou,
/// quando o objeto é trazido de outro arquivo, pelo nome dele em `via`: a
/// ligação a acha no [`Client`] com esse nome do arquivo de onde ele vem.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct RouteCall {
    pub method: String,
    pub path: String,
    pub written: String,
    pub line: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
    pub framework: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<RoutePath>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub via: String,
}

/// Um cliente que o arquivo faz e que outro arquivo pode trazer por import:
/// o nome que o recebe — vazio no que o arquivo exporta como padrão — e a
/// base dele, vazia quando ele não a escreve.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Client {
    pub framework: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(flatten)]
    pub base: RoutePath,
}

/// Um caminho de rota, ou um pedaço dele: como foi escrito, com os pedaços
/// juntados por barra, e padronizado como o [`Route::path`].
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct RoutePath {
    pub written: String,
    pub path: String,
}

/// A posição de um parâmetro numa declaração: o nome dela e a posição dele
/// entre os parâmetros, contada do zero sem o receptor; `None` é o receptor,
/// o parâmetro que recebe o objeto escrito antes do nome na chamada
/// (`api` em `api.MapOrders()`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpenPrefix {
    pub declaration: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<usize>,
}

/// Os prefixos que um arquivo escreve para rotas de outros arquivos.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteLinks {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<Mount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub globals: Vec<GlobalPrefix>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handoffs: Vec<Handoff>,
    /// Os clientes que o arquivo faz, com a base que eles põem na frente das
    /// chamadas feitas por eles noutros arquivos.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clients: Vec<Client>,
}

impl RouteLinks {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty() && self.globals.is_empty() && self.handoffs.is_empty() && self.clients.is_empty()
    }
}

/// A montagem de um nome que o arquivo não registra, ou que ele traz de
/// outro pelo import: o prefixo vale para as rotas do arquivo de onde o nome
/// vem. Com `whole`, o nome recebe o módulo
/// inteiro (o import padrão, o que recebe o `require`), e o prefixo vale
/// para todas as rotas dele; sem, só para as registradas num objeto com esse
/// nome ou escritas numa declaração com esse nome. A linha é a do nome, para
/// achar a declaração a que a chamada escrita ali liga. Com `module_path`, o
/// alvo é o caminho de um módulo escrito como texto, que se lê como import
/// do arquivo que o escreve; ele é sempre o módulo inteiro.
///
/// O prefixo já soma os que o arquivo põe por fora da montagem. O lugar em
/// que ela é feita — o objeto que a recebe (`receiver`) e a declaração em
/// que ela está escrita (`owner`) — é por onde outra montagem, de outro
/// arquivo, chega a ela: o prefixo posto nesse objeto ou nessa declaração
/// vem na frente do dela. Quando ela está no fim de outras montagens do
/// arquivo, `through` guarda os lugares da montagem mais de fora delas, como
/// em [`Route::through`]: a montagem de outro arquivo num deles também chega
/// a ela.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mount {
    pub framework: String,
    pub target: String,
    pub line: usize,
    #[serde(default, skip_serializing_if = "is_false")]
    pub whole: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub module_path: bool,
    #[serde(flatten)]
    pub prefix: RoutePath,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub receiver: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub through: Vec<String>,
}

/// O prefixo de todas as rotas do framework no projeto do arquivo que o
/// escreve (a pasta do manifesto mais perto acima dele), fora as rotas que
/// uma das exclusões tira.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct GlobalPrefix {
    pub framework: String,
    #[serde(flatten)]
    pub prefix: RoutePath,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excludes: Vec<Exclusion>,
}

/// Uma rota que o prefixo global não alcança: a de caminho igual a `path`,
/// ou que começa por ele com `starts`, do método `method` ([`ANY_METHOD`]
/// para qualquer um).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Exclusion {
    pub path: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub starts: bool,
    pub method: String,
}

impl Exclusion {
    /// A exclusão tira o prefixo da rota de método `method` e caminho `path`.
    #[must_use]
    pub fn covers(&self, method: &str, path: &str) -> bool {
        let same_path = if self.starts { path.starts_with(&self.path) } else { path == self.path };
        same_path && (self.method == ANY_METHOD || self.method == method)
    }
}

/// A entrega de um grupo: a chamada ao nome `name` que leva, na posição
/// `position` ([`OpenPrefix::position`]), um grupo com o prefixo dado. Com
/// `from`, o grupo nasce de um parâmetro da declaração em que a chamada
/// está, e o prefixo dele soma, na frente, o que chega a esse parâmetro.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Handoff {
    pub framework: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<usize>,
    #[serde(flatten)]
    pub prefix: RoutePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<OpenPrefix>,
}

/// O método da rota que atende qualquer método HTTP.
pub const ANY_METHOD: &str = "*";

impl Module {
    /// A linha cai num trecho de teste do arquivo ([`Module::test_lines`]).
    pub fn is_test_line(&self, line: usize) -> bool {
        self.test_lines.iter().any(|&(first, last)| (first..=last).contains(&line))
    }

    /// Em quantos módulos escritos dentro do arquivo
    /// ([`Module::module_lines`]) a linha cai.
    pub fn module_depth(&self, line: usize) -> usize {
        self.module_lines.iter().filter(|&&(first, last)| (first..=last).contains(&line)).count()
    }

    /// Dentro de quantos módulos do arquivo o import é escrito, uma vez por
    /// profundidade diferente ([`Module::import_lines`]). Contam só as linhas
    /// do trecho de teste quando `in_test`, e só as de fora dele no resto,
    /// como o import se reparte entre `test_imports` e `imports`. Zero quando
    /// o import só é escrito fora de qualquer módulo.
    pub fn import_depths(&self, import: &str, in_test: bool) -> BTreeSet<usize> {
        let depths: BTreeSet<usize> = self
            .import_lines
            .get(import)
            .into_iter()
            .flatten()
            .filter(|&&line| self.is_test_line(line) == in_test)
            .map(|&line| self.module_depth(line))
            .collect();
        if depths.is_empty() { BTreeSet::from([0]) } else { depths }
    }
}

/// One call or citation read out of a file: the name, the line, and the
/// qualifier written right before it (`q` in `q::name` and `q.name`, empty
/// when there is none, [`RECEIVER`] when what comes before is a value and not
/// a name). The caller is the file it was read from, and the
/// declaration that encloses the line — resolved by
/// [`crate::graph::link_declarations`], not stored twice.
///
/// Written as one string, `name:line`, or `q.name:line` when there is a
/// qualifier: there are tens of thousands of these, and the model is written
/// indented, so an object of three fields would cost six lines each. The map
/// is read by machine, and `name:line` is the form every reader already knows.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct CallSite {
    pub name: String,
    pub line: usize,
    /// The name written right before `::` or `.` ahead of this one. A name is
    /// never written with a dot, so the text splits back without ambiguity.
    pub qualifier: String,
}

/// O qualificador da chamada feita sobre um valor e não sobre um nome: o
/// resultado de outra chamada, um índice, um texto (`f().g()`, `a[0].g()`),
/// ou o nome escrito antes de um separador que só liga método. Nenhum nome se
/// escreve assim, e a ligação por ele nunca se prova: sem saber o tipo do
/// valor, a chamada pode alcançar qualquer método com o nome.
pub const RECEIVER: &str = "?";

/// A marca do receptor escrito pelo nome sozinho (`_logger` em
/// `_logger.Log()`), sem tipo e sem ligação local, numa língua que chama o
/// membro do próprio objeto pelo nome sozinho: `?@_logger`, com os campos
/// depois dele (`?@_ctx.config`). O nome pode ser um campo do tipo em volta ou
/// um tipo (`Console.WriteLine()`); o grafo vê qual pelo mapa.
pub const BARE: &str = "@";

impl Serialize for CallSite {
    fn serialize<S: Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        if self.qualifier.is_empty() {
            out.collect_str(&format_args!("{}:{}", self.name, self.line))
        } else {
            out.collect_str(&format_args!("{}.{}:{}", self.qualifier, self.name, self.line))
        }
    }
}

impl<'de> Deserialize<'de> for CallSite {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let text = String::deserialize(input)?;
        let (head, line) = text
            .rsplit_once(':')
            .ok_or_else(|| D::Error::custom(format!("a call site reads `name:line`, not `{text}`")))?;
        let line = line.parse().map_err(D::Error::custom)?;
        let (qualifier, name) = head.rsplit_once('.').unwrap_or(("", head));
        Ok(Self { name: name.to_string(), line, qualifier: qualifier.to_string() })
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A história sem commit, sem branch de partida e sem motivo: nada a
/// guardar.
fn is_blank_history(history: &History) -> bool {
    *history == History::default()
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Where the last pass read from, so the next one reads only what changed.
/// Which scanner build wrote each part of the map is the mark of its block
/// ([`ProjectModel::marks`]).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct ScanState {
    /// The commit checked out at the last pass (empty outside git and
    /// before the first commit).
    pub head: String,
    /// A branch de partida que a última passada leu, pelo `mustard.json`;
    /// vazia quando o projeto não declara uma.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub base: String,
    /// A ponta dessa branch na última passada: onde a história parou, e de
    /// onde a próxima soma os commits novos. Vazia sem a branch no clone.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub base_tip: String,
    /// A marca da listagem do git que a passada leu: a conferência antes de
    /// cada pergunta ao mapa a compara com a de agora, sem ler o mapa.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub listing: String,
    /// O blob de cada arquivo que decide a releitura sem ser código: os
    /// manifestos, os que mudam a leitura de todos os outros e os que não se
    /// decodificaram, pelo caminho.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, String>,
    /// Source files that could not be decoded, so an unchanged one is counted
    /// without being opened again.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub non_utf8: Vec<String>,
    /// O teto do nome comum com que a passada ligou as chamadas: a passada
    /// seguinte com outro teto religa o projeto, mesmo sem arquivo a reler.
    pub max_same_name: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Decl {
    pub kind: String,
    pub name: String,
    pub line: usize,
    /// The last line of the declaration's node — so a caller can point to the
    /// current start..end of the whole function/struct/etc without
    /// recomputing it. `0` when the extractor could not resolve it (older
    /// models default here too; additive field).
    #[serde(default)]
    pub end_line: usize,
    /// Names this declaration builds on — base classes, implemented interfaces,
    /// embedded structs, implemented traits. Language-specific to capture,
    /// generic to keep.
    #[serde(default)]
    pub supertypes: Vec<String>,
    /// The documentation comment written right above the declaration, cleaned
    /// of its comment markers and joined into one line. Empty when there is
    /// none there — and in this project it is the only part written in the
    /// developer's own language, so it is what makes a question in words meet
    /// the code. Additive: older models default to empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub doc: String,
    /// The declaration's own signature: what comes before its body, whitespace
    /// collapsed (name, parameters, return/base types). Never the body — the
    /// whole body was measured as the worst thing to keep. Empty when the
    /// declaration has no header to speak of.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signature: String,
    /// A documentação de cima inteira, sem o teto de [`Decl::doc`]: guardada
    /// só quando o teto cortou alguma coisa, e vazia quando a de `doc` já é a
    /// inteira.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub whole_doc: String,
    /// Os comentários escritos nas linhas da declaração, da primeira à
    /// última, limpos das marcas e juntados numa linha. Os de uma declaração
    /// de dentro são também da que a contém.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body_comment: String,
    /// Os nomes escritos no código das linhas da declaração, fora de
    /// comentário e de texto fixo, cada um uma vez, na ordem em que aparecem,
    /// separados por espaço.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body_names: String,
    /// The declarations of this project that this one calls, by name, sorted
    /// and deduped. Filled by [`crate::graph::link_declarations`] from the
    /// call sites of the file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<String>,
    /// Every use of this declaration: which file, which line, and which
    /// declaration the call starts from, proven or suspect. Filled by
    /// [`crate::graph::link_declarations`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used_by: Vec<UseSite>,
    /// Quantas chamadas pelo nome ficaram sem ligação por ele ser comum
    /// demais: cada uma podia alcançar mais declarações que o teto. Preenchido
    /// por [`crate::graph::link_declarations`].
    #[serde(default, skip_serializing_if = "is_zero")]
    pub common_calls: usize,
    /// Os donos da declaração, do mais interno para o mais externo: as
    /// declarações do mesmo arquivo cuja faixa contém a dela e, depois, o
    /// tipo escrito fora dela (`@owner`). Só os nomes, lidos com o arquivo:
    /// voltam do mapa com o arquivo que não mudou.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner: Vec<String>,
    /// O contrato que a declaração cumpre por onde foi escrita
    /// (`@owner.contract`), só os nomes, lidos com o arquivo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contract: Vec<String>,
    /// Num tipo, as declarações que o têm como dono mais interno, os métodos
    /// primeiro. Preenchido por [`crate::graph::link_members`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<DeclAt>,
    /// Num método, o método de mesmo nome do contrato que ele cumpre.
    /// Preenchido por [`crate::graph::link_members`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implements: Vec<DeclAt>,
    /// Num método de contrato, os métodos que o cumprem. Preenchido por
    /// [`crate::graph::link_members`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implemented_by: Vec<DeclAt>,
}

/// One use of a declaration, `file:line:from`, and one declaration a link
/// points to, `file:line:name`. The types live in the core, next to the map
/// questions that read them, so the side that writes the map and the side that
/// answers from it understand the same text.
pub use mustard_core::domain::project_map::{DeclAt, UseSite};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct GraphStats {
    pub nodes: usize,
    pub edges: usize,
    pub cyclic: bool,
    pub top_fan_in: Vec<NodeDegree>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct NodeDegree {
    pub module: String,
    pub degree: usize,
}
