//! O mapa do projeto, gravado pelo scan: a porta única de todo acesso ao
//! arquivo dele — o nome, o caminho, a pergunta "o mapa existe", a leitura e
//! a gravação. Ninguém mais escreve o nome do arquivo nem o abre direto. As
//! perguntas ao mapa moram em `domain::project_map`.
//!
//! O mapa é um banco SQLite só (`.claude/grain.db`), em blocos
//! ([`crate::io::map_db`]). Os blocos, as tabelas e as colunas se declaram
//! aqui, uma vez ([`BLOCKS`]): o scan grava por esta porta e as perguntas
//! leem por ela, e nenhum dos dois escreve o nome de uma tabela ou de uma
//! coluna.
//!
//! Cada coluna guarda uma chave do mapa em JSON — o formato que o scan
//! produz e que o [`ProjectMap`] lê —, e a porta passa de um para o outro nos
//! dois sentidos: a chave que falta fica vazia (`NULL`) e volta faltando; a
//! lista e o objeto se guardam como texto JSON. A chave que nenhuma coluna
//! guarda não entra no banco. Quem grava entrega o mapa em JSON e quem lê o
//! recebe de volta igual, sem saber das tabelas.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{params_from_iter, Connection};
use serde_json::{Map, Value};

use crate::domain::project_map::{
    Commit, History, MapDecl, MapDegree, MapLanguage, MapModule, MapProject, MapRefusal, MapSkeleton, ProjectMap,
};
use crate::io::map_db::{self, Block, Kind, MapDb};
use crate::platform::error::{Error, Result};

/// A pasta do projeto onde o mapa mora.
const MAP_DIR: &str = ".claude";

/// Onde o scan grava o mapa, a partir da raiz do projeto, com barras normais.
/// É o texto que as recusas, os avisos e as listas do censo citam.
pub const MAP_FILE: &str = ".claude/grain.db";

/// O nome do arquivo do mapa, sem a pasta: o que as listas de arquivos de
/// dentro de `.claude/` citam. Sai de [`MAP_FILE`], que é o único lugar do
/// nome.
pub const MAP_FILE_NAME: &str = MAP_FILE.split_at(MAP_DIR.len() + 1).1;

/// O diário que o SQLite cria ao lado do mapa enquanto uma gravação dura, e
/// apaga quando ela termina: o nome do mapa com `-journal` no fim.
pub const MAP_JOURNAL_FILE_NAME: &str = "grain.db-journal";

/// O mapa de antes do banco, em JSON. O scan o apaga depois de gravar o
/// banco; o nome fica para as listas do censo, que tratam o sumiço dele como
/// saída da ferramenta.
pub const LEGACY_MAP_FILE_NAME: &str = "grain.model.json";

/// O começo de todo arquivo SQLite.
const SQLITE_HEADER: &[u8] = b"SQLite format 3\0";

/// Onde o scan grava o mapa, dentro da raiz do projeto.
#[must_use]
pub fn model_path(root: &Path) -> PathBuf {
    root.join(MAP_DIR).join(MAP_FILE_NAME)
}

/// `true` quando há um mapa gravado em `model`: o caminho de [`model_path`],
/// ou o que o scan recebeu para gravar.
#[must_use]
pub fn exists_at(model: &Path) -> bool {
    model.is_file()
}

// ---------------------------------------------------------------------------
// Blocos, tabelas e colunas
// ---------------------------------------------------------------------------

/// Como uma coluna guarda a chave dela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    /// Um número inteiro.
    Int,
    /// Sim ou não, guardado como 1 ou 0.
    Flag,
    /// Um texto.
    Text,
    /// Qualquer valor — em geral lista ou objeto —, guardado como texto JSON.
    Json,
    /// O caminho do arquivo dono da linha, na tabela das declarações.
    Owner,
}

impl Cell {
    fn what(self) -> &'static str {
        match self {
            Self::Int => "a whole number",
            Self::Flag => "true or false",
            Self::Text | Self::Owner => "a text",
            Self::Json => "a JSON value",
        }
    }
}

/// Uma coluna: o nome dela e o caminho da chave que ela guarda, a partir do
/// objeto de onde sai a linha. O caminho vazio é o próprio item da lista.
#[derive(Debug)]
struct Column {
    name: &'static str,
    cell: Cell,
    key: &'static [&'static str],
}

/// De onde saem as linhas de uma tabela, no mapa em JSON.
#[derive(Debug, Clone, Copy)]
enum Place {
    /// Uma linha só, com chaves soltas do mapa; nenhuma quando todas faltam.
    One,
    /// Uma linha por item da lista em `at`. `keep` diz se a lista vazia volta
    /// como `[]` ou fica fora do mapa, como o scan a grava.
    List { at: &'static [&'static str], keep: bool },
    /// Uma linha por arquivo da lista `modules`, com o caminho dele na
    /// primeira coluna. A primeira tabela assim diz os arquivos e a ordem
    /// deles; as outras juntam chaves ao arquivo do mesmo caminho.
    Files,
    /// Uma linha por declaração de cada arquivo, com o caminho do arquivo na
    /// primeira coluna.
    Decls,
}

/// A lista em `at`, que volta como `[]` quando está vazia.
const fn list(at: &'static [&'static str]) -> Place {
    Place::List { at, keep: true }
}

/// Uma tabela do mapa.
#[derive(Debug)]
struct Table {
    name: &'static str,
    place: Place,
    columns: &'static [Column],
}

/// Um bloco do mapa: o que o banco sabe dele ([`Block`]) e onde cada tabela
/// dele mora no mapa em JSON.
#[derive(Debug)]
pub struct MapBlock {
    block: Block,
    tables: &'static [Table],
}

impl MapBlock {
    /// O nome do bloco na tabela de blocos do banco.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.block.name
    }
}

/// O tipo SQL de cada jeito de guardar, para o esquema.
macro_rules! sql_type {
    (Int) => {
        "INTEGER"
    };
    (Flag) => {
        "INTEGER"
    };
    (Text) => {
        "TEXT"
    };
    (Json) => {
        "TEXT"
    };
    (Owner) => {
        "TEXT"
    };
}

/// O caminho da chave de uma coluna: o dado entre colchetes, ou o próprio
/// nome da coluna.
macro_rules! key {
    ($column:literal) => {
        &[$column]
    };
    ($column:literal [$($part:literal),*]) => {
        &[$($part),*]
    };
}

/// Um bloco, declarado uma vez: o nome, a versão do formato e cada tabela,
/// com o lugar dela no mapa em JSON e as colunas. O esquema SQL sai da mesma
/// declaração.
macro_rules! block {
    ($name:literal, version $version:literal, {
        $( $table:literal at $place:expr => [
            $first:literal $first_cell:ident $([$($first_key:literal),*])?
            $(, $column:literal $cell:ident $([$($key:literal),*])?)*
        ] ),+
    }) => {
        MapBlock {
            block: Block {
                name: $name,
                version: $version,
                tables: &[$($table),+],
                schema: concat!($(
                    "CREATE TABLE ", $table, "(", $first, " ", sql_type!($first_cell)
                    $(, ", ", $column, " ", sql_type!($cell))*, ");"
                ),+),
                kind: Kind::Rebuilt(filled_by_the_scan),
            },
            tables: &[$(Table {
                name: $table,
                place: $place,
                columns: &[
                    Column { name: $first, cell: Cell::$first_cell, key: key!($first $([$($first_key),*])?) }
                    $(, Column { name: $column, cell: Cell::$cell, key: key!($column $([$($key),*])?) })*
                ],
            }),+],
        }
    };
}

/// Todo bloco do mapa é do scan: refeito, ele volta vazio e sem marca, e a
/// passada seguinte do scan o enche de novo, inteiro — ela lê tudo quando a
/// marca de um bloco que ela reaproveita não é a dela.
#[allow(clippy::unnecessary_wraps)] // a assinatura é a de todo bloco refeito
fn filled_by_the_scan(_: &Connection, _: &Path) -> Result<()> {
    Ok(())
}

/// O censo: o estado da leitura, as pilhas, os projetos, as línguas, os
/// manifestos e o esqueleto das pastas. O estado guarda o commit lido, a
/// marca da listagem do git daquela passada ([`Listing::digest`]) e o blob
/// de cada arquivo que decide a releitura sem ser código: os manifestos, os
/// que mudam a leitura de todos os outros e os que não se decodificaram.
pub const CENSUS: MapBlock = block!("census", version 2, {
    "census" at Place::One => [
        "root" Text,
        "head" Text ["state", "head"],
        "listing" Text ["state", "listing"],
        "inputs" Json ["state", "inputs"],
        "non_utf8" Json ["state", "non_utf8"],
        "frameworks" Json,
        "detected_stacks" Json,
        "skipped_build_dirs" Json ["coverage", "skipped_build_dirs"]
    ],
    "projects" at list(&["projects"]) => [
        "name" Text, "dir" Text, "kind" Text, "code_files" Int,
        "frameworks" Json, "dependencies" Json, "scripts" Json, "detected_stacks" Json
    ],
    "languages" at list(&["languages"]) => ["language" Text, "files" Int, "loc" Int],
    "manifests" at list(&["manifests"]) => [
        "path" Text, "kind" Text, "dependencies" Json, "scripts" Json, "name" Text, "module" Text, "package" Text
    ],
    "skeleton" at list(&["skeleton"]) => ["dir" Text, "role" Text]
});

/// Os arquivos: o que a passada que não relê um arquivo toma do mapa para
/// ele, fora as declarações e as ligações, e o blob do git do conteúdo que
/// ela leu.
pub const FILES: MapBlock = block!("files", version 2, {
    "files" at Place::Files => [
        "path" Text, "blob" Text, "language" Text, "loc" Int, "file_class" Text, "marker" Text, "has_tests" Flag,
        "namespaces" Json, "imports" Json, "global_imports" Json, "test_imports" Json,
        "test_lines" Json, "module_lines" Json, "import_lines" Json, "signals" Json
    ]
});

/// As declarações e os textos delas: o arquivo, o tipo, o nome, as linhas, a
/// assinatura, a documentação e quem usa cada uma; o dono e o contrato
/// escritos com ela; os membros de cada tipo e as implementações de cada
/// método, que a passada refaz do projeto inteiro como refaz os usos.
pub const DECLS: MapBlock = block!("decls", version 2, {
    "decls" at Place::Decls => [
        "file" Owner ["path"], "kind" Text, "name" Text, "line" Int, "end_line" Int,
        "signature" Text, "doc" Text, "supertypes" Json, "calls" Json, "used_by" Json,
        "owner" Json, "contract" Json, "members" Json, "implements" Json, "implemented_by" Json
    ]
});

/// O grafo: as importações resolvidas, os testes que cobrem cada arquivo, as
/// chamadas e as citações com a linha, e os arquivos mais importados.
pub const GRAPH: MapBlock = block!("graph", version 1, {
    "links" at Place::Files => [
        "path" Text, "deps" Json, "test_deps" Json, "tests" Json, "calls" Json, "cites" Json, "call_paths" Json
    ],
    "graph" at Place::One => ["nodes" Int ["graph", "nodes"], "edges" Int ["graph", "edges"]],
    "fan_in" at list(&["graph", "top_fan_in"]) => ["module" Text, "degree" Int],
    "layers" at list(&["graph", "layers"]) => ["name" Text, "modules" Int],
    "touchpoints" at list(&["graph", "touchpoints"]) => ["module" Text, "fan_out" Int, "breadth" Int]
});

/// A história do git: os caminhos numa tabela, em ordem, e os commits
/// apontando para ela.
pub const HISTORY: MapBlock = block!("history", version 1, {
    "history_paths" at Place::List { at: &["history", "paths"], keep: false } => ["path" Text []],
    "commits" at Place::List { at: &["history", "commits"], keep: false } => [
        "id" Text, "at" Int, "added" Json, "changed" Json
    ]
});

/// Os blocos do mapa, na ordem em que se leem: os arquivos antes das
/// declarações e das ligações deles.
pub const BLOCKS: [MapBlock; 5] = [CENSUS, FILES, DECLS, GRAPH, HISTORY];

/// Os mesmos blocos, como o banco os abre.
const DB_BLOCKS: [Block; 5] = [CENSUS.block, FILES.block, DECLS.block, GRAPH.block, HISTORY.block];

/// As chaves da lista dos arquivos e da lista das declarações de cada um.
const MODULES: &[&str] = &["modules"];
const DECLARATIONS: &[&str] = &["declarations"];

// ---------------------------------------------------------------------------
// Leitura
// ---------------------------------------------------------------------------

/// O mapa do projeto em `root`. Sem o arquivo, [`MapRefusal::MapMissing`];
/// com um arquivo que não se entende, [`MapRefusal::MapUnreadable`].
pub fn read(root: &Path) -> std::result::Result<ProjectMap, MapRefusal> {
    read_at(&model_path(root))
}

/// O mapa gravado em `model`, com as mesmas recusas de [`read`]. É a leitura
/// do mapa que o scan acabou de gravar num caminho escolhido por quem o
/// chamou.
pub fn read_at(model: &Path) -> std::result::Result<ProjectMap, MapRefusal> {
    let stored = read_stored_at(model)?;
    serde_json::from_str(&stored.json).map_err(|e| MapRefusal::MapUnreadable { detail: e.to_string() })
}

/// O mapa gravado, no formato em JSON do scan, com a marca de quem encheu
/// cada bloco.
#[derive(Debug, Clone, Default)]
pub struct StoredMap {
    /// O mapa em texto JSON, só com as chaves que as colunas guardam. Quem lê
    /// o passa direto ao tipo que quer: a coluna guardada em JSON entra no
    /// texto como está, sem ser lida duas vezes.
    pub json: String,
    /// A marca de cada bloco, pelo nome dele: vazia no bloco que ninguém
    /// marcou depois que nasceu ou foi refeito.
    pub marks: BTreeMap<String, String>,
}

/// O mapa gravado em `model` como o scan o escreveu, com as mesmas recusas
/// de [`read`]: o que a passada seguinte do scan toma da anterior, e o que os
/// testes dele conferem. Sem o arquivo, nada se cria.
pub fn read_stored_at(model: &Path) -> std::result::Result<StoredMap, MapRefusal> {
    let db = open_existing(model)?;
    let json = map_text(db.conn()).map_err(unreadable)?;
    let mut marks = BTreeMap::new();
    for block in &BLOCKS {
        if let Some(mark) = db.mark(block.name()).map_err(unreadable)? {
            marks.insert(block.name().to_string(), mark);
        }
    }
    Ok(StoredMap { json, marks })
}

/// O mapa do projeto em `root`, tabela por tabela, para depurar: primeiro as
/// tabelas dos blocos, na ordem em que se declaram, e depois as outras do
/// arquivo — a de blocos e as de um programa mais novo —, em ordem de nome.
/// Cada entrada traz o nome da tabela e as linhas, uma por objeto, com a
/// coluna vazia como `null` e o texto JSON já lido.
pub fn dump(root: &Path) -> std::result::Result<Value, MapRefusal> {
    let db = open_existing(&model_path(root))?;
    dump_tables(db.conn()).map_err(unreadable)
}

fn dump_tables(conn: &Connection) -> Result<Value> {
    let mut out = Vec::new();
    for table in BLOCKS.iter().flat_map(|block| block.tables) {
        let rows = rows_in(conn, table)?
            .iter()
            .map(|row| {
                let mut object = Map::new();
                for (column, value) in table.columns.iter().zip(row) {
                    object.insert(column.name.to_string(), json_of(table, column, value)?.unwrap_or(Value::Null));
                }
                Ok(Value::Object(object))
            })
            .collect::<std::result::Result<Vec<_>, String>>()
            .map_err(Error::Parse)?;
        out.push(serde_json::json!({ "table": table.name, "rows": rows }));
    }
    let declared: Vec<&str> = BLOCKS.iter().flat_map(|block| block.tables.iter().map(|table| table.name)).collect();
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
    let others: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter(|name| !declared.contains(&name.as_str()))
        .collect();
    for name in others {
        let mut stmt = conn.prepare(&format!("SELECT * FROM {}", quoted(&name)))?;
        let columns: Vec<String> = stmt.column_names().iter().map(|column| (*column).to_string()).collect();
        let rows = stmt
            .query_map([], |row| {
                let mut object = Map::new();
                for (at, column) in columns.iter().enumerate() {
                    object.insert(column.clone(), plain_json(row.get::<_, Sql>(at)?));
                }
                Ok(Value::Object(object))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        out.push(serde_json::json!({ "table": name, "rows": rows }));
    }
    Ok(Value::Array(out))
}

/// O valor de uma tabela que a porta não declara, como ele está.
fn plain_json(value: Sql) -> Value {
    match value {
        Sql::Null => Value::Null,
        Sql::Integer(n) => Value::from(n),
        Sql::Real(x) => Value::from(x),
        Sql::Text(text) => Value::String(text),
        Sql::Blob(bytes) => Value::String(format!("<{} bytes>", bytes.len())),
    }
}

// ---------------------------------------------------------------------------
// Leitura por pergunta
// ---------------------------------------------------------------------------

/// O que uma pergunta ao mapa precisa ler. Cada uma lê só as tabelas dela,
/// direto das linhas, sem montar o mapa inteiro: o [`ProjectMap`] que volta
/// traz só as partes que a pergunta usa, e as perguntas de
/// `domain::project_map` respondem dele o mesmo que responderiam do mapa
/// inteiro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need<'a> {
    /// Nada além de o mapa se abrir: só as recusas de mapa ausente e de
    /// mapa ilegível.
    Nothing,
    /// O resumo do início da sessão: o caminho de cada arquivo, as línguas,
    /// os subprojetos, os arquivos mais importados e a história.
    Summary,
    /// O terreno do início da sessão: os subprojetos e a camada de cada
    /// pasta.
    Terrain,
    /// Os caminhos dos arquivos, e nada mais deles.
    Paths,
    /// Quem importa o arquivo: ele e os arquivos que o importam, com as
    /// importações de cada um.
    Importers(&'a str),
    /// Os testes do arquivo: ele, com os testes que o cobrem e a marca dos
    /// próprios testes.
    Tests(&'a str),
    /// As declarações com o nome, com o arquivo delas; com `file`, só as
    /// desse arquivo, que vem mesmo sem nenhuma.
    Declarations { file: Option<&'a str>, name: &'a str },
    /// Os exemplos para uma tarefa: cada arquivo com o tamanho, a classe, os
    /// testes e as importações, e a história. Com `words`, também os nomes
    /// que cada arquivo declara, que a busca da pasta usa.
    Examples { words: bool },
}

/// O mapa do projeto em `root` com só o que `need` pede, com as mesmas
/// recusas de [`read`].
pub fn read_for(root: &Path, need: Need<'_>) -> std::result::Result<ProjectMap, MapRefusal> {
    let db = open_existing(&model_path(root))?;
    part_of(db.conn(), need).map_err(unreadable)
}

fn part_of(conn: &Connection, need: Need<'_>) -> Result<ProjectMap> {
    use crate::domain::project_map::clean_path;
    let mut map = ProjectMap::default();
    match need {
        Need::Nothing => {}
        Need::Summary => {
            map.modules = file_rows(conn, &["path"], None)?.into_iter().map(module_of).collect::<Result<_>>()?;
            map.languages = languages(conn)?;
            map.projects = projects(conn)?;
            map.graph.top_fan_in = fan_in(conn)?;
            map.history = history(conn)?;
        }
        Need::Terrain => {
            map.projects = projects(conn)?;
            map.skeleton = skeleton(conn)?;
        }
        Need::Paths => {
            map.modules = file_rows(conn, &["path"], None)?.into_iter().map(module_of).collect::<Result<_>>()?;
        }
        Need::Importers(file) => map.modules = importers(conn, &clean_path(file))?,
        Need::Tests(file) => map.modules = tests_of(conn, &clean_path(file))?,
        Need::Declarations { file, name } => {
            map.modules = named(conn, file.map(clean_path).as_deref(), name.trim())?;
        }
        Need::Examples { words } => {
            map.modules = example_modules(conn, words)?;
            map.history = history(conn)?;
        }
    }
    Ok(map)
}

/// O nome entre aspas de uma tabela declarada; a tabela que nenhum bloco
/// declara é recusada.
fn table_name(table: &str) -> Result<String> {
    declared_table(table).map(|found| quoted(found.name))
}

fn declared_table(table: &str) -> Result<&'static Table> {
    BLOCKS
        .iter()
        .flat_map(|block| block.tables)
        .find(|found| found.name == table)
        .ok_or_else(|| Error::Parse(format!("the map declares no table `{table}`")))
}

/// As colunas `columns` da tabela `table`, entre aspas e com o nome da
/// tabela na frente, separadas por vírgula; a coluna que a tabela não
/// declara é recusada.
fn column_names(table: &str, columns: &[&str]) -> Result<String> {
    let found = declared_table(table)?;
    let names = columns
        .iter()
        .map(|column| {
            if found.columns.iter().any(|declared| declared.name == *column) {
                Ok(format!("{}.{}", quoted(found.name), quoted(column)))
            } else {
                Err(Error::Parse(format!("the map table `{table}` declares no column `{column}`")))
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(names.join(", "))
}

/// Uma linha lida, com as colunas pedidas na ordem em que se pediram.
type Picked = Vec<Sql>;

/// As colunas `columns` de cada linha de `table`, na ordem em que entraram,
/// só das linhas que `filter` (o `WHERE`, com `params`) deixa.
fn picked(conn: &Connection, table: &str, columns: &[&str], filter: &str, params: &[&str]) -> Result<Vec<Picked>> {
    let name = table_name(table)?;
    let filter = if filter.is_empty() { String::new() } else { format!(" WHERE {filter}") };
    let sql = format!("SELECT {} FROM {name}{filter} ORDER BY {name}.rowid", column_names(table, columns)?);
    let mut stmt = conn.prepare(&sql)?;
    let width = columns.len();
    let rows = stmt
        .query_map(params_from_iter(params), |row| (0..width).map(|at| row.get::<_, Sql>(at)).collect::<rusqlite::Result<Picked>>())?
        .collect::<rusqlite::Result<Vec<Picked>>>()?;
    Ok(rows)
}

/// As colunas `columns` das linhas da tabela dos arquivos: de todas, ou só
/// da do caminho `path`.
fn file_rows(conn: &Connection, columns: &[&str], path: Option<&str>) -> Result<Vec<Picked>> {
    match path {
        None => picked(conn, "files", columns, "", &[]),
        Some(path) => picked(conn, "files", columns, &format!("{} = ?1", column_names("files", &["path"])?), &[path]),
    }
}

/// O texto de uma célula; vazio quando ela está vazia.
fn text_cell(value: &Sql) -> String {
    match value {
        Sql::Text(text) => text.clone(),
        _ => String::new(),
    }
}

/// O número de uma célula; zero quando ela está vazia.
fn int_cell(value: &Sql) -> i64 {
    match value {
        Sql::Integer(n) => *n,
        _ => 0,
    }
}

/// O valor guardado em JSON numa célula, lido como `T`; o padrão quando
/// ela está vazia.
fn json_cell<T: serde::de::DeserializeOwned + Default>(value: &Sql) -> Result<T> {
    match value {
        Sql::Text(text) => serde_json::from_str(text).map_err(|e| Error::Parse(format!("map cell holds broken JSON: {e}"))),
        _ => Ok(T::default()),
    }
}

/// O arquivo de uma linha que só traz o caminho.
fn module_of(row: Picked) -> Result<MapModule> {
    Ok(MapModule { path: row.first().map(text_cell).unwrap_or_default(), ..MapModule::default() })
}

fn languages(conn: &Connection) -> Result<Vec<MapLanguage>> {
    picked(conn, "languages", &["language", "files", "loc"], "", &[])?
        .iter()
        .map(|row| Ok(MapLanguage { language: text_cell(&row[0]), files: int_cell(&row[1]) as usize, loc: int_cell(&row[2]) as usize }))
        .collect()
}

fn projects(conn: &Connection) -> Result<Vec<MapProject>> {
    picked(conn, "projects", &["name", "dir", "kind", "code_files"], "", &[])?
        .iter()
        .map(|row| {
            Ok(MapProject {
                name: text_cell(&row[0]),
                dir: text_cell(&row[1]),
                kind: text_cell(&row[2]),
                code_files: int_cell(&row[3]) as usize,
            })
        })
        .collect()
}

fn skeleton(conn: &Connection) -> Result<Vec<MapSkeleton>> {
    picked(conn, "skeleton", &["dir", "role"], "", &[])?
        .iter()
        .map(|row| Ok(MapSkeleton { dir: text_cell(&row[0]), role: text_cell(&row[1]) }))
        .collect()
}

fn fan_in(conn: &Connection) -> Result<Vec<MapDegree>> {
    picked(conn, "fan_in", &["module", "degree"], "", &[])?
        .iter()
        .map(|row| Ok(MapDegree { module: text_cell(&row[0]), degree: int_cell(&row[1]) as usize }))
        .collect()
}

fn history(conn: &Connection) -> Result<History> {
    let paths = picked(conn, "history_paths", &["path"], "", &[])?.iter().map(|row| text_cell(&row[0])).collect();
    let commits = picked(conn, "commits", &["id", "at", "added", "changed"], "", &[])?
        .iter()
        .map(|row| {
            Ok(Commit { id: text_cell(&row[0]), at: int_cell(&row[1]), added: json_cell(&row[2])?, changed: json_cell(&row[3])? })
        })
        .collect::<Result<_>>()?;
    Ok(History { paths, commits })
}

/// O arquivo `file` e os que o importam, na ordem do mapa, cada um com as
/// importações dele.
fn importers(conn: &Connection, file: &str) -> Result<Vec<MapModule>> {
    let files = table_name("files")?;
    let links = table_name("links")?;
    let (path, link_path) = (column_names("files", &["path"])?, column_names("links", &["path"])?);
    let deps = column_names("links", &["deps"])?;
    let sql = format!(
        "SELECT {path}, {deps} FROM {files} LEFT JOIN {links} ON {link_path} = {path} \
         WHERE {path} = ?1 OR EXISTS (SELECT 1 FROM json_each({deps}) WHERE json_each.value = ?1) ORDER BY {files}.rowid"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([file], |row| Ok((row.get::<_, Sql>(0)?, row.get::<_, Sql>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (path, deps) = row?;
        out.push(MapModule { path: text_cell(&path), deps: json_cell(&deps)?, ..MapModule::default() });
    }
    Ok(out)
}

/// O arquivo `file`, com a marca dos próprios testes e os testes que o
/// cobrem.
fn tests_of(conn: &Connection, file: &str) -> Result<Vec<MapModule>> {
    let Some(row) = file_rows(conn, &["path", "has_tests"], Some(file))?.into_iter().next() else { return Ok(Vec::new()) };
    let tests = picked(conn, "links", &["tests"], &format!("{} = ?1", column_names("links", &["path"])?), &[file])?;
    Ok(vec![MapModule {
        path: text_cell(&row[0]),
        has_tests: int_cell(&row[1]) != 0,
        tests: tests.first().map(|row| json_cell(&row[0])).transpose()?.unwrap_or_default(),
        ..MapModule::default()
    }])
}

/// As colunas de uma declaração que as perguntas pelo nome leem.
const NAMED_COLUMNS: [&str; 13] = [
    "file", "kind", "name", "line", "end_line", "doc", "signature", "used_by",
    "owner", "contract", "members", "implements", "implemented_by",
];

/// A declaração de uma linha com as colunas de [`NAMED_COLUMNS`].
fn named_decl(row: &Picked) -> Result<MapDecl> {
    Ok(MapDecl {
        kind: text_cell(&row[1]),
        name: text_cell(&row[2]),
        line: int_cell(&row[3]) as u64,
        end_line: int_cell(&row[4]) as u64,
        doc: text_cell(&row[5]),
        signature: text_cell(&row[6]),
        used_by: json_cell(&row[7])?,
        owner: json_cell(&row[8])?,
        contract: json_cell(&row[9])?,
        members: json_cell(&row[10])?,
        implements: json_cell(&row[11])?,
        implemented_by: json_cell(&row[12])?,
    })
}

/// As declarações chamadas `name`, cada uma no arquivo dela, na ordem do
/// mapa. Com `file`, só as desse arquivo, que vem mesmo sem nenhuma; o
/// arquivo que o mapa não tem não vem.
fn named(conn: &Connection, file: Option<&str>, name: &str) -> Result<Vec<MapModule>> {
    let decl_file = column_names("decls", &["file"])?;
    let decl_name = column_names("decls", &["name"])?;
    let Some(file) = file else {
        let rows = picked(conn, "decls", &NAMED_COLUMNS, &format!("{decl_name} = ?1"), &[name])?;
        let owners: BTreeSet<String> = rows.iter().map(|row| text_cell(&row[0])).collect();
        let mut modules: Vec<MapModule> = Vec::new();
        if !owners.is_empty() {
            for row in file_rows(conn, &["path"], None)? {
                if owners.contains(&text_cell(&row[0])) {
                    modules.push(module_of(row)?);
                }
            }
        }
        for row in &rows {
            let owner = text_cell(&row[0]);
            if let Some(module) = modules.iter_mut().find(|module| module.path == owner) {
                module.declarations.push(named_decl(row)?);
            }
        }
        return Ok(modules);
    };
    let Some(row) = file_rows(conn, &["path"], Some(file))?.into_iter().next() else { return Ok(Vec::new()) };
    let mut module = module_of(row)?;
    for row in picked(conn, "decls", &NAMED_COLUMNS, &format!("{decl_file} = ?1 AND {decl_name} = ?2"), &[file, name])? {
        module.declarations.push(named_decl(&row)?);
    }
    Ok(vec![module])
}

/// Cada arquivo com o que os exemplos leem dele: o tamanho, a classe, a
/// marca dos próprios testes, as importações e os testes que o cobrem; com
/// `words`, também os nomes que ele declara.
fn example_modules(conn: &Connection, words: bool) -> Result<Vec<MapModule>> {
    let mut modules: Vec<MapModule> = file_rows(conn, &["path", "loc", "file_class", "has_tests"], None)?
        .iter()
        .map(|row| MapModule {
            path: text_cell(&row[0]),
            loc: int_cell(&row[1]) as usize,
            file_class: text_cell(&row[2]),
            has_tests: int_cell(&row[3]) != 0,
            ..MapModule::default()
        })
        .collect();
    let mut at: HashMap<String, usize> = HashMap::new();
    for (index, module) in modules.iter().enumerate() {
        at.entry(module.path.clone()).or_insert(index);
    }
    for row in picked(conn, "links", &["path", "deps", "tests"], "", &[])? {
        if let Some(&index) = at.get(&text_cell(&row[0])) {
            modules[index].deps = json_cell(&row[1])?;
            modules[index].tests = json_cell(&row[2])?;
        }
    }
    if words {
        for row in picked(conn, "decls", &["file", "name"], "", &[])? {
            if let Some(&index) = at.get(&text_cell(&row[0])) {
                modules[index].declarations.push(MapDecl { name: text_cell(&row[1]), ..MapDecl::default() });
            }
        }
    }
    Ok(modules)
}

// ---------------------------------------------------------------------------
// O conteúdo do projeto, pelo git
// ---------------------------------------------------------------------------

/// Quantos caminhos vão numa chamada só ao git que calcula blobs.
const HASH_BATCH: usize = 256;

/// O modo que o índice do git dá a um submódulo.
const SUBMODULE_MODE: &str = "160000";

/// Cada arquivo do projeto como está agora, pelo git: o commit do checkout e
/// o id do blob do conteúdo de cada arquivo, pelo caminho relativo à pasta
/// lida. O arquivo comitado e intocado vem do índice; o mudado, o novo e o
/// que só está no índice vêm do mesmo cálculo sobre o conteúdo de agora. O
/// próprio mapa e o diário dele ficam de fora: senão cada gravação dele
/// mudaria a listagem.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// O commit do checkout; vazio num repositório sem commit.
    pub head: String,
    /// O blob de cada arquivo, pelo caminho.
    pub blobs: BTreeMap<String, String>,
}

impl Listing {
    /// Uma marca curta e estável de todos os pares caminho e blob: duas
    /// listagens com a mesma marca têm os mesmos arquivos com os mesmos
    /// conteúdos.
    #[must_use]
    pub fn digest(&self) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for (path, blob) in &self.blobs {
            for byte in path.bytes().chain([0]).chain(blob.bytes()).chain([0]) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
        format!("{hash:016x}-{}", self.blobs.len())
    }
}

/// O git em `root`, com os caminhos escritos como são; `None` quando ele
/// falha ou falta.
fn git_out(root: &Path, args: &[&str]) -> Option<String> {
    let mut full = vec!["-c", "core.quotePath=false"];
    full.extend(args);
    let run = crate::platform::git::run(root, &full);
    run.ok.then_some(run.stdout)
}

/// O conteúdo do projeto em `root` agora, pelo git. `None` fora do git.
#[must_use]
pub fn listing(root: &Path) -> Option<Listing> {
    let own = [MAP_FILE_NAME, MAP_JOURNAL_FILE_NAME].map(|name| format!("{MAP_DIR}/{name}"));
    let blobs = blobs_under(root, &own)?;
    let head = git_out(root, &["rev-parse", "--verify", "-q", "HEAD"]).map(|out| out.trim().to_string()).unwrap_or_default();
    Some(Listing { head, blobs })
}

/// O blob de cada arquivo sob `root`, com os de dentro dos submódulos
/// iniciados, pelo caminho relativo a `root`, fora os caminhos `skip`, que
/// nem se calculam.
fn blobs_under(root: &Path, skip: &[String]) -> Option<BTreeMap<String, String>> {
    let staged = git_out(root, &["ls-files", "-s", "-z"])?;
    let mut blobs = BTreeMap::new();
    let mut nested = Vec::new();
    for entry in staged.split('\0') {
        let Some((meta, path)) = entry.split_once('\t') else { continue };
        let mut parts = meta.split(' ');
        let (Some(mode), Some(blob)) = (parts.next(), parts.next()) else { continue };
        if mode == SUBMODULE_MODE {
            nested.push(path.to_string());
        } else if !skip.iter().any(|own| own == path) {
            blobs.insert(path.to_string(), blob.to_string());
        }
    }
    let prefix = git_out(root, &["rev-parse", "--show-prefix"])?.trim().to_string();
    let status = git_out(root, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."])?;
    let mut fresh = Vec::new();
    let mut fields = status.split('\0');
    while let Some(entry) = fields.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(3);
        // A troca de nome e a cópia trazem o caminho antigo no campo seguinte.
        if code[..2].contains(['R', 'C']) {
            let _ = fields.next();
        }
        // Os caminhos da situação partem do topo do repositório.
        let Some(rel) = path.strip_prefix(prefix.as_str()) else { continue };
        if skip.iter().any(|own| own == rel) {
            blobs.remove(rel);
        } else if root.join(rel).is_file() {
            fresh.push(rel.to_string());
        } else {
            blobs.remove(rel);
        }
    }
    for batch in fresh.chunks(HASH_BATCH) {
        let mut args = vec!["hash-object", "--"];
        args.extend(batch.iter().map(String::as_str));
        let hashed = git_out(root, &args)?;
        for (path, blob) in batch.iter().zip(hashed.lines()) {
            blobs.insert(path.clone(), blob.trim().to_string());
        }
    }
    for sub in nested {
        let dir = root.join(&sub);
        if !dir.join(".git").exists() {
            continue;
        }
        for (path, blob) in blobs_under(&dir, &[]).unwrap_or_default() {
            blobs.insert(format!("{sub}/{path}"), blob);
        }
    }
    Some(blobs)
}

/// O mapa de `root` ficou atrás do conteúdo de agora: o commit do checkout
/// ou algum arquivo mudou desde a passada que o gravou. Lê só o estado
/// gravado, nunca o mapa inteiro. `false` sem mapa, com um mapa que não se
/// lê e fora do git: não há com que comparar.
#[must_use]
pub fn is_behind(root: &Path) -> bool {
    let Ok(db) = open_existing(&model_path(root)) else { return false };
    let Ok(rows) = picked(db.conn(), "census", &["head", "listing"], "", &[]) else { return false };
    let Some(now) = listing(root) else { return false };
    let (head, digest) = rows.first().map_or_else(Default::default, |row| (text_cell(&row[0]), text_cell(&row[1])));
    head != now.head || digest != now.digest()
}

/// O banco em `model`, que já tem de existir: sem o arquivo, a recusa de
/// mapa ausente, e nada se cria. O arquivo que não começa como um banco —
/// vazio, ou com o texto que não se entendeu — é recusado antes de abrir: o
/// SQLite tomaria o arquivo curto por um banco novo e gravaria nele.
fn open_existing(model: &Path) -> std::result::Result<MapDb, MapRefusal> {
    if !exists_at(model) {
        return Err(MapRefusal::MapMissing);
    }
    if head_of(model) != SQLITE_HEADER {
        return Err(MapRefusal::MapUnreadable { detail: format!("{} is not a SQLite database", model.display()) });
    }
    open(model).map_err(unreadable)
}

/// Abre o banco em `model` com os blocos do mapa em dia.
fn open(model: &Path) -> Result<MapDb> {
    // A raiz do projeto só serve ao bloco refeito, e todo bloco do mapa volta
    // vazio para o scan encher: a pasta de cima basta.
    let root = model.parent().and_then(Path::parent).unwrap_or_else(|| Path::new("."));
    MapDb::open(model, root, &DB_BLOCKS)
}

fn unreadable(err: Error) -> MapRefusal {
    MapRefusal::MapUnreadable { detail: err.to_string() }
}

/// As linhas de uma tabela, na ordem em que entraram.
type Row = Vec<Sql>;

fn rows_in(conn: &Connection, table: &Table) -> Result<Vec<Row>> {
    let columns: Vec<String> = table.columns.iter().map(|column| quoted(column.name)).collect();
    let mut stmt = conn.prepare(&format!("SELECT {} FROM {} ORDER BY rowid", columns.join(", "), quoted(table.name)))?;
    let width = table.columns.len();
    let rows = stmt
        .query_map([], |row| (0..width).map(|at| row.get::<_, Sql>(at)).collect::<rusqlite::Result<Row>>())?
        .collect::<rusqlite::Result<Vec<Row>>>()?;
    Ok(rows)
}

/// Um pedaço do mapa em JSON: um texto JSON já pronto, a lista dos arquivos,
/// ou um objeto em volta deles, para as chaves soltas do mapa, que o caminho
/// da coluna aninha.
enum Node {
    Raw(String),
    Modules(Vec<ModuleText>),
    Object(Vec<(&'static str, Node)>),
}

impl Node {
    /// Um teto do tamanho do pedaço em texto, para o texto do mapa nascer do
    /// tamanho dele, sem crescer aos saltos.
    fn size(&self) -> usize {
        match self {
            Self::Raw(text) => text.len(),
            Self::Modules(modules) => modules
                .iter()
                .map(|module| module.fields.len() + module.declarations.len() + DECLARATIONS[0].len() + 8)
                .sum::<usize>()
                + 2,
            Self::Object(entries) => entries.iter().map(|(key, node)| key.len() + 4 + node.size()).sum::<usize>() + 2,
        }
    }

    /// Escreve o pedaço em `out`, como texto JSON. As chaves são nomes
    /// declarados nesta porta, sem nada a escapar.
    fn write(&self, out: &mut String) {
        match self {
            Self::Raw(text) => out.push_str(text),
            Self::Modules(modules) => {
                out.push('[');
                for (at, module) in modules.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    out.push('{');
                    out.push_str(&module.fields);
                    if !module.fields.is_empty() {
                        out.push(',');
                    }
                    push_key(out, DECLARATIONS[0]);
                    out.push('[');
                    out.push_str(&module.declarations);
                    out.push_str("]}");
                }
                out.push(']');
            }
            Self::Object(entries) => {
                out.push('{');
                for (at, (key, node)) in entries.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    push_key(out, key);
                    node.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// Um objeto do mapa, com as chaves na ordem em que entraram.
type Object = Vec<(&'static str, Node)>;

/// O arquivo do mapa enquanto o texto se monta: as chaves dele, das tabelas
/// por arquivo, e as declarações, cada parte já em texto JSON.
#[derive(Default)]
struct ModuleText {
    fields: String,
    declarations: String,
}

/// O mapa em texto JSON, escrito direto das linhas de cada tabela, na ordem
/// dos blocos: as chaves soltas, as listas e os arquivos com as declarações de
/// cada um. A coluna em JSON entra como está; o texto que não se lê é recusado
/// por quem lê o mapa inteiro.
fn map_text(conn: &Connection) -> Result<String> {
    let mut top: Object = Vec::new();
    let mut modules: Vec<ModuleText> = Vec::new();
    let mut by_path: HashMap<Option<String>, usize> = HashMap::new();
    let mut files_table: Option<&str> = None;
    for table in BLOCKS.iter().flat_map(|block| block.tables) {
        let columns: Vec<String> = table.columns.iter().map(|column| quoted(column.name)).collect();
        let mut stmt = conn.prepare(&format!("SELECT {} FROM {} ORDER BY rowid", columns.join(", "), quoted(table.name)))?;
        let mut rows = stmt.query([])?;
        match table.place {
            Place::One => {
                while let Some(row) = rows.next()? {
                    for (at, column) in table.columns.iter().enumerate() {
                        let mut text = String::new();
                        if push_cell(&mut text, table, column, row.get_ref(at)?)? {
                            put(&mut top, column.key, Node::Raw(text));
                        }
                    }
                }
            }
            Place::List { at, keep } => {
                let mut list = String::from("[");
                let mut empty = true;
                while let Some(row) = rows.next()? {
                    if !empty {
                        list.push(',');
                    }
                    empty = false;
                    push_item(&mut list, table, row)?;
                }
                list.push(']');
                if keep || !empty {
                    put(&mut top, at, Node::Raw(list));
                }
            }
            Place::Files => {
                let first = *files_table.get_or_insert(table.name) == table.name;
                while let Some(row) = rows.next()? {
                    let path = owner_of(row)?;
                    let module = match by_path.get(&path) {
                        Some(&at) => &mut modules[at],
                        None if first => {
                            by_path.insert(path, modules.len());
                            modules.push(ModuleText::default());
                            let Some(module) = modules.last_mut() else { continue };
                            module
                        }
                        // A ligação de um arquivo que a tabela dos arquivos
                        // não tem fica de fora.
                        None => continue,
                    };
                    // Na tabela dos arquivos o caminho é chave do próprio
                    // arquivo; nas outras por arquivo, só diz a que arquivo a
                    // linha se junta.
                    let comma = !module.fields.is_empty();
                    push_fields(&mut module.fields, table, row, usize::from(!first), comma)?;
                }
            }
            Place::Decls => {
                while let Some(row) = rows.next()? {
                    let Some(&at) = by_path.get(&owner_of(row)?) else { continue };
                    let declarations = &mut modules[at].declarations;
                    if !declarations.is_empty() {
                        declarations.push(',');
                    }
                    declarations.push('{');
                    push_fields(declarations, table, row, 1, false)?;
                    declarations.push('}');
                }
            }
        }
    }
    put(&mut top, MODULES, Node::Modules(modules));
    let top = Node::Object(top);
    let mut out = String::with_capacity(top.size());
    top.write(&mut out);
    Ok(out)
}

/// O caminho do arquivo dono da linha, na primeira coluna de uma tabela por
/// arquivo; `None` quando a coluna está vazia.
fn owner_of(row: &rusqlite::Row<'_>) -> Result<Option<String>> {
    Ok(match row.get_ref(0)? {
        ValueRef::Text(text) => Some(String::from_utf8_lossy(text).into_owned()),
        _ => None,
    })
}

/// Escreve em `out` as chaves de uma linha, sem as chaves em volta, a partir
/// da coluna `skip`, separadas por vírgula; a primeira também, quando
/// `comma`. As colunas de uma linha de lista ou de arquivo têm nome de um
/// nível só.
fn push_fields(out: &mut String, table: &Table, row: &rusqlite::Row<'_>, skip: usize, mut comma: bool) -> Result<()> {
    for (at, column) in table.columns.iter().enumerate().skip(skip) {
        let value = row.get_ref(at)?;
        if value == ValueRef::Null {
            continue;
        }
        if comma {
            out.push(',');
        }
        comma = true;
        push_key(out, column.key.last().copied().unwrap_or(column.name));
        push_cell(out, table, column, value)?;
    }
    Ok(())
}

/// Escreve em `out` um item de lista: o próprio valor, na tabela de uma
/// coluna só com o caminho vazio; senão, o objeto das colunas.
fn push_item(out: &mut String, table: &Table, row: &rusqlite::Row<'_>) -> Result<()> {
    if let [column] = table.columns
        && column.key.is_empty()
    {
        if !push_cell(out, table, column, row.get_ref(0)?)? {
            out.push_str("null");
        }
        return Ok(());
    }
    out.push('{');
    push_fields(out, table, row, 0, false)?;
    out.push('}');
    Ok(())
}

/// Escreve `"key":` em `out`.
fn push_key(out: &mut String, key: &str) {
    out.push('"');
    out.push_str(key);
    out.push_str("\":");
}

/// Escreve em `out` o valor de uma coluna como texto JSON; `false`, sem
/// escrever nada, quando ela está vazia.
fn push_cell(out: &mut String, table: &Table, column: &Column, value: ValueRef<'_>) -> Result<bool> {
    use std::fmt::Write as _;
    let text = |bytes| {
        std::str::from_utf8(bytes).map_err(|e| Error::Parse(format!("`{}.{}` is not UTF-8: {e}", table.name, column.name)))
    };
    match (column.cell, value) {
        (_, ValueRef::Null) => return Ok(false),
        (Cell::Int, ValueRef::Integer(n)) => {
            let _ = write!(out, "{n}");
        }
        (Cell::Flag, ValueRef::Integer(n)) => out.push_str(if n == 0 { "false" } else { "true" }),
        (Cell::Text | Cell::Owner, ValueRef::Text(bytes)) => push_string(out, text(bytes)?),
        (Cell::Json, ValueRef::Text(bytes)) => out.push_str(text(bytes)?),
        _ => {
            return Err(Error::Parse(format!("`{}.{}` does not hold {}", table.name, column.name, column.cell.what())));
        }
    }
    Ok(true)
}

/// Escreve `text` em `out` como texto JSON, entre aspas e escapado. O que
/// não se escapa vai em trechos inteiros; o byte que se escapa é ASCII e
/// nunca cai no meio de uma letra de mais bytes.
fn push_string(out: &mut String, text: &str) {
    use std::fmt::Write as _;
    out.push('"');
    let mut start = 0;
    for (at, byte) in text.bytes().enumerate() {
        let escaped = match byte {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0..=0x1f => "",
            _ => continue,
        };
        out.push_str(&text[start..at]);
        if escaped.is_empty() {
            let _ = write!(out, "\\u{byte:04x}");
        } else {
            out.push_str(escaped);
        }
        start = at + 1;
    }
    out.push_str(&text[start..]);
    out.push('"');
}

/// A chave `key` de `object` passa a valer `value`, criando os objetos do
/// caminho que faltam.
fn put(object: &mut Object, key: &[&'static str], value: Node) {
    let Some((last, parents)) = key.split_last() else { return };
    let mut at = object;
    for part in parents {
        let index = match at.iter().position(|(name, _)| name == part) {
            Some(index) => index,
            None => {
                at.push((part, Node::Object(Vec::new())));
                at.len() - 1
            }
        };
        if !matches!(at[index].1, Node::Object(_)) {
            at[index].1 = Node::Object(Vec::new());
        }
        let Node::Object(inner) = &mut at[index].1 else { return };
        at = inner;
    }
    match at.iter_mut().find(|(name, _)| name == last) {
        Some(entry) => entry.1 = value,
        None => at.push((last, value)),
    }
}

/// O valor de uma coluna em JSON; `None` quando ela está vazia.
fn json_of(table: &Table, column: &Column, value: &Sql) -> std::result::Result<Option<Value>, String> {
    Ok(Some(match (column.cell, value) {
        (_, Sql::Null) => return Ok(None),
        (Cell::Int, Sql::Integer(n)) => Value::from(*n),
        (Cell::Flag, Sql::Integer(n)) => Value::Bool(*n != 0),
        (Cell::Text | Cell::Owner, Sql::Text(text)) => Value::String(text.clone()),
        (Cell::Json, Sql::Text(text)) => {
            serde_json::from_str(text).map_err(|e| format!("`{}.{}` holds broken JSON: {e}", table.name, column.name))?
        }
        _ => return Err(format!("`{}.{}` does not hold {}", table.name, column.name, column.cell.what())),
    }))
}

// ---------------------------------------------------------------------------
// Gravação
// ---------------------------------------------------------------------------

/// Grava `map` — o mapa em JSON, no formato do scan — no banco em `model`,
/// com a marca `mark` em cada bloco. Só o bloco cujas linhas ou cuja marca
/// mudaram se regrava, e todos os que mudaram numa transação só: quem lê
/// nunca vê um pela metade. Devolve `true` quando gravou; com tudo igual,
/// `false`, e o arquivo fica como estava. Um arquivo em `model` que não é um
/// banco é trocado pelo mapa.
///
/// # Errors
///
/// A chave que não cabe na coluna dela ([`Error::Parse`]) e a falha do banco.
pub fn save_at(model: &Path, map: &Value, mark: &str) -> Result<bool> {
    let fresh = rows_of(map).map_err(Error::Parse)?;
    save_rows(model, &fresh, mark)
}

/// As linhas de cada tabela de cada bloco, na ordem de [`BLOCKS`].
type BlockRows = Vec<Vec<Row>>;

fn save_rows(model: &Path, fresh: &[BlockRows], mark: &str) -> Result<bool> {
    let head = head_of(model);
    if !head.is_empty() && head != SQLITE_HEADER {
        remove(model)?;
    }
    let mut db = open(model)?;
    let mut changed: Vec<(&MapBlock, &BlockRows)> = Vec::new();
    for (block, tables) in BLOCKS.iter().zip(fresh) {
        let mut same = db.mark(block.name())?.as_deref() == Some(mark);
        for (table, rows) in block.tables.iter().zip(tables) {
            if !same {
                break;
            }
            same = rows_in(db.conn(), table)? == *rows;
        }
        if !same {
            changed.push((block, tables));
        }
    }
    if changed.is_empty() {
        return Ok(false);
    }
    db.write(|tx| {
        for (block, tables) in &changed {
            for (table, rows) in block.tables.iter().zip(tables.iter()) {
                tx.execute(&format!("DELETE FROM {}", quoted(table.name)), [])?;
                let names: Vec<String> = table.columns.iter().map(|column| quoted(column.name)).collect();
                let slots = vec!["?"; names.len()].join(", ");
                let mut insert =
                    tx.prepare(&format!("INSERT INTO {}({}) VALUES ({slots})", quoted(table.name), names.join(", ")))?;
                for row in rows {
                    insert.execute(params_from_iter(row))?;
                }
            }
            map_db::set_mark(tx, block.name(), mark)?;
        }
        Ok(())
    })?;
    Ok(true)
}

/// As linhas que `map` dá a cada tabela de cada bloco.
fn rows_of(map: &Value) -> std::result::Result<Vec<BlockRows>, String> {
    BLOCKS
        .iter()
        .map(|block| block.tables.iter().map(|table| rows(table, map)).collect())
        .collect()
}

fn rows(table: &Table, map: &Value) -> std::result::Result<Vec<Row>, String> {
    let row = |item: &Value, owner: &Value| -> std::result::Result<Row, String> {
        table
            .columns
            .iter()
            .map(|column| {
                let from = if column.cell == Cell::Owner { owner } else { item };
                cell(table, column, find(from, column.key))
            })
            .collect()
    };
    match table.place {
        Place::One => {
            let only = row(map, map)?;
            Ok(if only.iter().all(|value| *value == Sql::Null) { Vec::new() } else { vec![only] })
        }
        Place::List { at, .. } => items(map, at)?.iter().map(|item| row(item, map)).collect(),
        Place::Files => items(map, MODULES)?.iter().map(|module| row(module, module)).collect(),
        Place::Decls => {
            let mut out = Vec::new();
            for module in items(map, MODULES)? {
                for decl in items(module, DECLARATIONS)? {
                    out.push(row(decl, module)?);
                }
            }
            Ok(out)
        }
    }
}

/// O valor no caminho `key` a partir de `value`; o próprio valor com o
/// caminho vazio.
fn find<'a>(value: &'a Value, key: &[&str]) -> Option<&'a Value> {
    key.iter().try_fold(value, |value, part| value.get(part))
}

/// A lista no caminho `at`: vazia quando ela falta; recusada quando o valor
/// não é uma lista.
fn items<'a>(value: &'a Value, at: &[&str]) -> std::result::Result<&'a [Value], String> {
    match find(value, at) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(format!("`{}` is not a list", at.join("."))),
    }
}

/// O valor de `value` na coluna: vazio quando falta; recusado quando não é
/// do jeito que a coluna guarda.
fn cell(table: &Table, column: &Column, value: Option<&Value>) -> std::result::Result<Sql, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else { return Ok(Sql::Null) };
    let wrong = || format!("`{}.{}` is not {}: {value}", table.name, column.name, column.cell.what());
    match column.cell {
        Cell::Int => value.as_i64().map(Sql::Integer).ok_or_else(wrong),
        Cell::Flag => value.as_bool().map(|flag| Sql::Integer(i64::from(flag))).ok_or_else(wrong),
        Cell::Text | Cell::Owner => value.as_str().map(|text| Sql::Text(text.to_string())).ok_or_else(wrong),
        Cell::Json => Ok(Sql::Text(value.to_string())),
    }
}

/// Grava `map` como o mapa do projeto em `root`, no lugar do que houver,
/// criando a pasta quando ela falta. É como os testes deixam um mapa pronto
/// sem rodar o scan.
///
/// # Errors
///
/// A falha de gravação do disco.
pub fn write(root: &Path, map: &ProjectMap) -> Result<()> {
    write_text(root, &serde_json::to_string(map)?)
}

/// Grava `text` como o mapa do projeto em `root`, no lugar do que houver,
/// criando a pasta quando ela falta: é o mapa escrito à mão num teste, com o
/// formato inteiro do scan, ou um que não se entende.
///
/// # Errors
///
/// A falha de gravação do disco.
pub fn write_text(root: &Path, text: &str) -> Result<()> {
    write_text_at(&model_path(root), text)
}

/// Grava `text` como o mapa em `model`, como [`write_text`]. O mapa em JSON
/// vai para o banco, sem marca em bloco nenhum; o texto que não se entende
/// fica no arquivo como veio, que não é um banco: a leitura o recusa como
/// mapa ilegível, como o mapa estragado.
///
/// # Errors
///
/// A falha de gravação do disco.
pub fn write_text_at(model: &Path, text: &str) -> Result<()> {
    remove(model)?;
    let understood = serde_json::from_str::<Value>(text).ok().filter(Value::is_object).and_then(|map| rows_of(&map).ok());
    match understood {
        Some(fresh) => save_rows(model, &fresh, "").map(|_| ()),
        None => crate::io::fs::write_atomic(model, text.as_bytes()),
    }
}

/// Apaga o mapa em `model` e o diário ao lado dele, quando existem: o diário
/// que sobrasse seria aplicado ao mapa novo.
fn remove(model: &Path) -> Result<()> {
    let mut journal = model.as_os_str().to_owned();
    journal.push("-journal");
    for path in [model.to_path_buf(), PathBuf::from(journal)] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Os primeiros bytes do arquivo em `model`, até o tamanho do começo de todo
/// banco SQLite; vazio quando o arquivo falta ou está vazio.
fn head_of(model: &Path) -> Vec<u8> {
    use std::io::Read;
    let mut head = Vec::with_capacity(SQLITE_HEADER.len());
    if let Ok(file) = std::fs::File::open(model) {
        // O que não se lê conta como vazio: a abertura do banco dá o erro.
        let _ = file.take(SQLITE_HEADER.len() as u64).read_to_end(&mut head);
    }
    head
}

/// O nome de tabela ou de coluna entre aspas duplas.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::project_map::{DeclAt, MapModule, MapState};
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn a_missing_map_and_a_broken_map_are_told_apart() {
        let dir = tempdir().unwrap();
        assert!(!exists_at(&model_path(dir.path())), "nothing was written yet");
        assert_eq!(read(dir.path()).unwrap_err(), MapRefusal::MapMissing);
        write_text(dir.path(), "{not json").unwrap();
        assert!(exists_at(&model_path(dir.path())), "a broken map still exists");
        assert_eq!(read(dir.path()).unwrap_err().reason(), "map-unreadable");
        write_text(dir.path(), r#"{"modules":[{"path":"a.rs","deps":["b.rs"]}],"other":1}"#).unwrap();
        let map = read(dir.path()).unwrap();
        assert_eq!(map.modules[0].deps, vec!["b.rs".to_string()]);
    }

    /// Perguntar ao projeto sem mapa dá a recusa de mapa ausente e não deixa
    /// um banco vazio no lugar dele; o texto vazio e o JSON com o tipo errado
    /// são mapas que não se entendem.
    #[test]
    fn reading_a_project_without_a_map_refuses_and_creates_nothing() {
        let dir = tempdir().unwrap();
        for _ in 0..2 {
            assert_eq!(read(dir.path()).unwrap_err(), MapRefusal::MapMissing);
            assert_eq!(read_stored_at(&model_path(dir.path())).unwrap_err(), MapRefusal::MapMissing);
            assert_eq!(dump(dir.path()).unwrap_err(), MapRefusal::MapMissing);
        }
        assert!(!model_path(dir.path()).exists(), "reading created the map");
        assert!(!dir.path().join(MAP_DIR).exists(), "reading created the folder");

        for broken in ["", r#"{"modules":[{"path":"a.rs","loc":"many"}]}"#] {
            write_text(dir.path(), broken).unwrap();
            assert_eq!(read(dir.path()).unwrap_err().reason(), "map-unreadable", "{broken:?}");
        }
    }

    /// Um mapa gravado pela porta volta igual pela leitura dela, e só passa a
    /// existir depois da gravação.
    #[test]
    fn a_written_map_reads_back_the_same() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("projeto");
        assert!(!exists_at(&model_path(&root)));
        let map = ProjectMap {
            modules: vec![MapModule {
                path: "src/a.rs".to_string(),
                deps: vec!["src/b.rs".to_string()],
                ..MapModule::default()
            }],
            state: MapState { head: "abc123".to_string() },
            ..ProjectMap::default()
        };
        write(&root, &map).unwrap();
        assert!(exists_at(&model_path(&root)), "the map exists after it is written");
        let back = read(&root).unwrap();
        assert_eq!(back.modules.len(), 1);
        assert_eq!(back.modules[0].path, "src/a.rs");
        assert_eq!(back.modules[0].deps, vec!["src/b.rs".to_string()]);
        assert_eq!(back.state.head, "abc123");
        assert_eq!(read_at(&model_path(&root)).unwrap().state.head, "abc123");
    }

    /// O nome do arquivo e o caminho saem do mesmo texto, e o diário é o nome
    /// do mapa com o fim que o SQLite dá.
    #[test]
    fn the_file_name_and_the_path_come_from_one_text() {
        assert_eq!(format!("{MAP_DIR}/{MAP_FILE_NAME}"), MAP_FILE);
        assert!(model_path(Path::new("raiz")).ends_with(MAP_FILE));
        assert_eq!(MAP_JOURNAL_FILE_NAME, format!("{MAP_FILE_NAME}-journal"));
    }

    /// A listagem dá, pelo caminho relativo à pasta lida, o blob do que cada
    /// arquivo guarda agora: o do índice para o intocado, o do conteúdo para
    /// o editado e para o novo, nada para o apagado; o próprio mapa e o
    /// diário dele ficam de fora, e a marca muda com o conteúdo.
    #[test]
    fn the_listing_gives_the_blob_of_what_each_file_holds_now() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        let git = |args: &[&str]| -> String {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q"]);
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::write(repo.join("top.txt"), "fora\n").unwrap();
        std::fs::write(repo.join("sub/a.txt"), "a\n").unwrap();
        std::fs::write(repo.join("sub/gone.txt"), "g\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        let root = repo.join("sub");

        let first = listing(&root).expect("dentro do git");
        assert_eq!(first.head, git(&["rev-parse", "HEAD"]));
        assert_eq!(first.blobs.keys().collect::<Vec<_>>(), ["a.txt", "gone.txt"], "só o que mora na pasta lida");
        assert_eq!(first.blobs["a.txt"], git(&["hash-object", "sub/a.txt"]));

        std::fs::write(root.join("a.txt"), "a mudado\n").unwrap();
        std::fs::write(root.join("new.txt"), "novo\n").unwrap();
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        std::fs::create_dir_all(root.join(MAP_DIR)).unwrap();
        std::fs::write(root.join(MAP_DIR).join(MAP_FILE_NAME), "mapa").unwrap();
        std::fs::write(root.join(MAP_DIR).join(MAP_JOURNAL_FILE_NAME), "diário").unwrap();
        let now = listing(&root).expect("dentro do git");
        assert_eq!(now.blobs.keys().collect::<Vec<_>>(), ["a.txt", "new.txt"]);
        assert_eq!(now.blobs["a.txt"], git(&["hash-object", "sub/a.txt"]));
        assert_eq!(now.blobs["new.txt"], git(&["hash-object", "sub/new.txt"]));
        assert_ne!(now.digest(), first.digest());
        assert_eq!(now.digest(), listing(&root).unwrap().digest(), "a mesma listagem, a mesma marca");

        assert_eq!(listing(tempdir().unwrap().path()), None, "fora do git não há listagem");
    }

    /// Um mapa como o scan o grava: uma chave de cada jeito de guardar, em
    /// cada tabela, com lista vazia, chave que falta e chave que o banco não
    /// guarda.
    fn scan_map() -> Value {
        json!({
            "root": "/proj",
            "languages": [{"language": "rust", "files": 2, "loc": 30}],
            "manifests": [{"path": "Cargo.toml", "kind": "cargo", "dependencies": ["serde"], "scripts": [], "name": "demo"}],
            "frameworks": ["serde"],
            "skeleton": [{"dir": "src", "role": "L0"}],
            "modules": [
                {"path": "src/a.rs", "blob": "a1", "language": "rust", "loc": 10, "imports": [], "namespaces": [],
                 "test_lines": [[8, 10]], "import_lines": {"crate::b": [1]}, "has_tests": true,
                 "declarations": [
                    {"kind": "function", "name": "alpha", "line": 1, "end_line": 3, "supertypes": [],
                     "doc": "Soma um.", "signature": "fn alpha(s: &str) -> \"a\\b\"\n\t\u{1} ação", "used_by": ["src/b.rs:2:beta"]},
                    {"kind": "struct", "name": "Alpha", "line": 5, "end_line": 6, "supertypes": ["Base"],
                     "members": ["src/a.rs:7:run"]},
                    {"kind": "method", "name": "run", "line": 7, "end_line": 7, "owner": ["Alpha"], "contract": ["Base"],
                     "implements": ["src/b.rs:3:run"], "implemented_by": ["src/c.rs:9:run"]}
                 ],
                 "deps": ["src/b.rs"], "calls": ["beta:2", "b.beta:4"], "call_paths": {"crate::b": ["beta:4"]}},
                {"path": "src/b.rs", "language": "rust", "loc": 20, "imports": ["crate::a"], "namespaces": [],
                 "declarations": [], "file_class": "generated", "marker": "@generated"}
            ],
            "graph": {"nodes": 2, "edges": 1, "cyclic": false,
                      "top_fan_in": [{"module": "src/b.rs", "degree": 1}], "top_fan_out": [{"module": "src/a.rs", "degree": 1}],
                      "layers": [{"name": "L0", "modules": 2}], "touchpoints": []},
            "coverage": {"top_dirs": [], "skipped_build_dirs": ["target"], "code_files_read": 2},
            "projects": [{"name": "demo", "dir": "", "kind": "cargo", "code_files": 2, "frameworks": [], "dependencies": ["serde"],
                          "scripts": [], "detected_stacks": [{"stack": "rust", "confidence": 0.5}]}],
            "shared_contracts": [{"name": "Base", "implementors": 3}],
            "detected_stacks": [],
            "state": {"head": "abc", "listing": "00ff-2", "inputs": {"Cargo.toml": "b1"}},
            "history": {"paths": ["src/a.rs", "src/b.rs"], "commits": [{"id": "c1", "at": 10, "added": [0, 1]}, {"id": "c2", "at": 20, "changed": [1]}]}
        })
    }

    /// O mapa do scan volta do banco igual, fora as chaves que nenhuma coluna
    /// guarda.
    #[test]
    fn the_scan_map_comes_back_from_the_database_as_it_went_in() {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        let map = scan_map();
        assert!(save_at(&model, &map, "scan 1").unwrap());
        let stored = read_stored_at(&model).unwrap();
        let back: Value = serde_json::from_str(&stored.json).unwrap();

        let mut expected = map;
        let Value::Object(top) = &mut expected else { unreachable!() };
        top.remove("shared_contracts");
        top["graph"].as_object_mut().unwrap().retain(|key, _| !["cyclic", "top_fan_out"].contains(&key.as_str()));
        top["coverage"].as_object_mut().unwrap().retain(|key, _| key == "skipped_build_dirs");
        assert_eq!(back, expected);
        assert_eq!(stored.marks.len(), BLOCKS.len());
        assert!(stored.marks.values().all(|mark| mark == "scan 1"), "{:?}", stored.marks);

        // A leitura das perguntas vê o mesmo mapa.
        let read = read_at(&model).unwrap();
        assert_eq!(read.modules[0].declarations[0].used_by[0].file, "src/b.rs");
        // O dono, o contrato e as ligações de membro e de implementação voltam
        // com a declaração.
        let run = &read.modules[0].declarations[2];
        assert_eq!((run.owner.as_slice(), run.contract.as_slice()), (&["Alpha".to_string()][..], &["Base".to_string()][..]));
        assert_eq!(read.modules[0].declarations[1].members[0], DeclAt { file: "src/a.rs".into(), line: 7, name: "run".into() });
        assert_eq!(run.implements[0], DeclAt { file: "src/b.rs".into(), line: 3, name: "run".into() });
        assert_eq!(run.implemented_by[0].file, "src/c.rs");
        // A pergunta pelo nome lê as mesmas ligações só das colunas dela.
        let asked = read_for(dir.path(), Need::Declarations { file: None, name: "run" }).unwrap();
        assert_eq!(
            serde_json::to_value(&asked.modules[0].declarations[0]).unwrap(),
            serde_json::to_value(run).unwrap()
        );
        assert_eq!(read.graph.top_fan_in[0].module, "src/b.rs");
        assert_eq!(read.history.commits.len(), 2);
    }

    /// Com o mesmo mapa e a mesma marca, nada se grava: o arquivo fica com os
    /// mesmos bytes. Um arquivo que muda regrava o mapa; a marca nova, também.
    #[test]
    fn saving_the_same_map_again_writes_nothing() {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        let map = scan_map();
        assert!(save_at(&model, &map, "scan 1").unwrap());
        let before = std::fs::read(&model).unwrap();
        assert!(!save_at(&model, &map, "scan 1").unwrap(), "nothing changed, nothing is written");
        assert_eq!(std::fs::read(&model).unwrap(), before);

        let mut changed = map.clone();
        changed["modules"][1]["loc"] = json!(21);
        assert!(save_at(&model, &changed, "scan 1").unwrap());
        let back: Value = serde_json::from_str(&read_stored_at(&model).unwrap().json).unwrap();
        assert_eq!(back["modules"][1]["loc"], json!(21));
        assert!(save_at(&model, &changed, "scan 2").unwrap(), "a new mark is written");
        assert!(read_stored_at(&model).unwrap().marks.values().all(|mark| mark == "scan 2"));
    }

    /// O arquivo que não é um banco é trocado pelo mapa na gravação do scan.
    #[test]
    fn a_file_that_is_not_a_database_is_replaced_by_the_map() {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        write_text(dir.path(), "{not json").unwrap();
        assert!(save_at(&model, &scan_map(), "scan 1").unwrap());
        assert_eq!(read(dir.path()).unwrap().modules.len(), 2);
    }

    /// O despejo traz uma entrada por tabela, na ordem dos blocos, e as
    /// linhas de cada uma.
    #[test]
    fn the_dump_has_one_entry_per_table() {
        let dir = tempdir().unwrap();
        save_at(&model_path(dir.path()), &scan_map(), "scan 1").unwrap();
        let dump = dump(dir.path()).unwrap();
        let names: Vec<&str> = dump.as_array().unwrap().iter().map(|entry| entry["table"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "census", "projects", "languages", "manifests", "skeleton", "files", "decls", "links", "graph", "fan_in",
                "layers", "touchpoints", "history_paths", "commits", "blocks"
            ]
        );
        let decls = &dump[6]["rows"];
        assert_eq!(decls[0]["file"], json!("src/a.rs"));
        assert_eq!(decls[0]["used_by"], json!(["src/b.rs:2:beta"]));
        assert_eq!(decls[1]["doc"], Value::Null);
    }
}
