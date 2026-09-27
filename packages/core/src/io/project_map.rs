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

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{params_from_iter, Connection};
use serde_json::{Map, Value};

use crate::domain::project_map::{MapRefusal, ProjectMap};
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
/// manifestos e o esqueleto das pastas.
pub const CENSUS: MapBlock = block!("census", version 1, {
    "census" at Place::One => [
        "root" Text,
        "head" Text ["state", "head"],
        "dirty" Json ["state", "dirty"],
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
/// ele, fora as declarações e as ligações.
pub const FILES: MapBlock = block!("files", version 1, {
    "files" at Place::Files => [
        "path" Text, "language" Text, "loc" Int, "file_class" Text, "marker" Text, "has_tests" Flag,
        "namespaces" Json, "imports" Json, "global_imports" Json, "test_imports" Json,
        "test_lines" Json, "module_lines" Json, "import_lines" Json, "signals" Json
    ]
});

/// As declarações e os textos delas: o arquivo, o tipo, o nome, as linhas, a
/// assinatura, a documentação e quem usa cada uma.
pub const DECLS: MapBlock = block!("decls", version 1, {
    "decls" at Place::Decls => [
        "file" Owner ["path"], "kind" Text, "name" Text, "line" Int, "end_line" Int,
        "signature" Text, "doc" Text, "supertypes" Json, "calls" Json, "used_by" Json
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
    use crate::domain::project_map::{MapModule, MapState};
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
                {"path": "src/a.rs", "language": "rust", "loc": 10, "imports": [], "namespaces": [],
                 "test_lines": [[8, 10]], "import_lines": {"crate::b": [1]}, "has_tests": true,
                 "declarations": [
                    {"kind": "function", "name": "alpha", "line": 1, "end_line": 3, "supertypes": [],
                     "doc": "Soma um.", "signature": "fn alpha(s: &str) -> \"a\\b\"\n\t\u{1} ação", "used_by": ["src/b.rs:2:beta"]},
                    {"kind": "struct", "name": "Alpha", "line": 5, "end_line": 6, "supertypes": ["Base"]}
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
            "state": {"head": "abc", "dirty": ["src/a.rs"]},
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
