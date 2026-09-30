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
    Commit, DeclLineage, FileLineage, History, LineageCommit, MapDecl, MapDegree, MapLanguage, MapModule, MapProject,
    MapRefusal, MapSkeleton, ProjectMap, PullComment, PullOfCommit, PullText, Pulls,
};
use crate::domain::normalize::Languages;
use crate::io::map_db::{self, Block, Kind, MapDb};
use crate::io::{map_fill, map_format, map_glossary, map_revision, map_search};
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
/// apaga quando ela termina: o nome do mapa com `-journal` no fim. O mapa
/// grava em `WAL`; o diário de antes só aparece num mapa que um programa mais
/// velho ainda grave.
pub const MAP_JOURNAL_FILE_NAME: &str = "grain.db-journal";

/// O registro das gravações que o SQLite mantém ao lado do mapa enquanto há
/// conexão aberta: o nome do mapa com `-wal` no fim. A última conexão a
/// fechar o junta ao mapa e o apaga.
pub const MAP_WAL_FILE_NAME: &str = "grain.db-wal";

/// A memória compartilhada das conexões abertas do mapa: o nome do mapa com
/// `-shm` no fim. Some com a última conexão.
pub const MAP_SHARED_FILE_NAME: &str = "grain.db-shm";

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
///
/// O índice de busca do bloco, quando há, entra à parte: as tabelas dele e o
/// esquema em SQL. Ele não mora no mapa em JSON, não se compara na gravação
/// — a tabela sem o texto guardado não se lê de volta — e se refaz sempre
/// que o bloco se regrava; na troca de versão, sai junto das outras.
///
/// A tabela que saiu do bloco numa versão anterior entra em `retired`: fora
/// do esquema e do mapa em JSON, ela só sai do banco na troca de versão,
/// junto das outras, para o banco antigo não guardar o que ninguém mais lê.
macro_rules! block {
    ($name:literal, version $version:literal, {
        $( $table:literal at $place:expr => [
            $first:literal $first_cell:ident $([$($first_key:literal),*])?
            $(, $column:literal $cell:ident $([$($key:literal),*])?)*
        ] ),+
    } $(, index [$($index_table:literal),+] $index_schema:literal)? $(, retired [$($retired_table:literal),+])?) => {
        MapBlock {
            block: Block {
                name: $name,
                version: $version,
                tables: &[$($table,)+ $($($index_table,)+)? $($($retired_table,)+)?],
                schema: concat!($(
                    "CREATE TABLE ", $table, "(", $first, " ", sql_type!($first_cell)
                    $(, ", ", $column, " ", sql_type!($cell))*, ");"
                ),+ $(, $index_schema)?),
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
/// branch de partida e a ponta dela, onde a história parou, a marca da listagem
/// do git daquela passada ([`Listing::digest`]) e o blob
/// de cada arquivo que decide a releitura sem ser código: os manifestos, os
/// que mudam a leitura de todos os outros e os que não se decodificaram. E o
/// teto do nome comum com que as chamadas ligaram, que religa o projeto
/// quando muda.
pub const CENSUS: MapBlock = block!("census", version 5, {
    "census" at Place::One => [
        "root" Text,
        "head" Text ["state", "head"],
        "base" Text ["state", "base"],
        "base_tip" Text ["state", "base_tip"],
        "listing" Text ["state", "listing"],
        "inputs" Json ["state", "inputs"],
        "non_utf8" Json ["state", "non_utf8"],
        "max_same_name" Int ["state", "max_same_name"],
        "frameworks" Json,
        "detected_stacks" Json,
        "skipped_build_dirs" Json ["coverage", "skipped_build_dirs"]
    ],
    "projects" at list(&["projects"]) => [
        "name" Text, "dir" Text, "kind" Text, "code_files" Int,
        "frameworks" Json, "scripts" Json, "detected_stacks" Json
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
/// assinatura, a documentação — e a inteira, quando o teto a cortou —, os
/// comentários e os nomes escritos nas linhas dela, e quem usa cada uma —
/// cada uso provado ou suspeito, com as candidatas, e quantas chamadas do
/// nome ficaram sem ligação por ele ser comum demais; o dono e o contrato
/// escritos com ela; os membros de cada tipo e as implementações de cada
/// método, que a passada refaz do projeto inteiro como refaz os usos. Ao
/// lado, os textos fixos de cada arquivo — as mensagens de log, as de erro e
/// os outros textos escritos no código —, cada um com a linha, a marca e a
/// declaração que o contém, e os comentários do arquivo: os do começo e os
/// outros que caem fora das linhas de toda declaração — os de dentro moram
/// uma vez só, nos da declaração —, com quantos bytes do começo dos
/// comentários da primeira declaração de fora são também do começo do
/// arquivo; e as medidas de qualidade de cada arquivo, que a passada refaz
/// do projeto inteiro, porque a repetição e o ciclo dependem dos outros.
///
/// Junto delas mora o índice da busca do mapa ([`crate::io::map_search`]):
/// uma tabela FTS5 por nível — a declaração e o arquivo —, uma coluna por
/// campo e sem o texto guardado; a lista de cada forma pelo `fts5vocab`; o
/// tamanho de cada campo, em palavras; os nomes das declarações numa tabela
/// trigram, inteiros e dobrados (minúsculas, sem acento, só letras e
/// números), para o pedaço do nome; e as línguas e as médias com que ele foi
/// feito. Cada declaração leva também, nos dois últimos campos, os títulos
/// dos commits que a mudaram — a história por declaração que o mapa guarda
/// dos arquivos já lidos — e os nomes de quem a usa. As listas saem antes das tabelas de que elas leem. Os campos que a
/// busca sem filtro lê vêm primeiro; os do texto de dentro das peças vêm
/// depois, e só a busca com filtro os lê. A declaração de teste fica fora do
/// nível das declarações, e só a tabela trigram a guarda.
pub const DECLS: MapBlock = block!("decls", version 13, {
    "decls" at Place::Decls => [
        "file" Owner ["path"], "kind" Text, "name" Text, "line" Int, "end_line" Int,
        "signature" Text, "doc" Text, "whole_doc" Text, "body_comment" Text, "body_names" Text,
        "supertypes" Json, "calls" Json, "used_by" Json, "common_calls" Int,
        "owner" Json, "contract" Json, "members" Json, "implements" Json, "implemented_by" Json
    ],
    "texts" at Place::Files => [
        "path" Text, "texts" Json, "file_doc" Text, "file_comment" Text, "file_doc_in_body" Int, "quality" Json
    ]
}, index [
    "file_vocab", "decl_vocab", "file_fts", "decl_fts", "decl_trigram", "file_lengths", "decl_lengths", "search_meta"
] "CREATE VIRTUAL TABLE file_fts USING fts5(name, path, doc, log, error, text, file_doc, file_comment, commits, content='', \
     contentless_delete=1, tokenize='unicode61 remove_diacritics 2');\
   CREATE VIRTUAL TABLE decl_fts USING fts5(name, path, signature, doc, log, error, text, whole_doc, body_comment, \
     body_names, body_calls, owner, members, commits, callers, content='', contentless_delete=1, \
     tokenize='unicode61 remove_diacritics 2', prefix='3 4');\
   CREATE VIRTUAL TABLE file_vocab USING fts5vocab(file_fts, instance);\
   CREATE VIRTUAL TABLE decl_vocab USING fts5vocab(decl_fts, instance);\
   CREATE VIRTUAL TABLE decl_trigram USING fts5(name, folded, file UNINDEXED, tokenize='trigram');\
   CREATE TABLE file_lengths(id INTEGER PRIMARY KEY, name INTEGER, path INTEGER, doc INTEGER, log INTEGER, \
     error INTEGER, text INTEGER, file_doc INTEGER, file_comment INTEGER, commits INTEGER);\
   CREATE TABLE decl_lengths(id INTEGER PRIMARY KEY, name INTEGER, path INTEGER, signature INTEGER, doc INTEGER, \
     log INTEGER, error INTEGER, text INTEGER, whole_doc INTEGER, body_comment INTEGER, body_names INTEGER, \
     body_calls INTEGER, owner INTEGER, members INTEGER, commits INTEGER, callers INTEGER);\
   CREATE TABLE search_meta(key TEXT PRIMARY KEY, value);");

/// As rotas do servidor de cada arquivo: o método, o caminho padronizado e o
/// escrito, a função que atende cada uma e a linha dela, o framework cuja
/// regra a achou e as chamadas da tela que a alcançam, provadas ou
/// suspeitas; os prefixos que o arquivo escreve para rotas de outros
/// arquivos e os clientes que ele faz, que a passada seguinte soma de novo
/// sem reler o arquivo; e as chamadas da tela escritas no arquivo, que ela
/// liga de novo às rotas.
pub const ROUTES: MapBlock = block!("routes", version 3, {
    "routes" at Place::Files => ["path" Text, "routes" Json, "route_links" Json, "route_calls" Json]
});

/// O grafo: as importações resolvidas, os testes que cobrem cada arquivo, as
/// chamadas, as citações, os nomes escritos onde vai um valor e os membros
/// escritos depois do objeto, com a linha, os caminhos escritos antes das
/// chamadas, os nomes que cada import traz, cada um com o nome que tem no
/// arquivo de origem, os nomes que cada repasse oferece, os nomes que abrem a
/// cadeia de uma chamada ou de um membro sem que o arquivo os ligue, e os
/// arquivos mais importados.
pub const GRAPH: MapBlock = block!("graph", version 7, {
    "links" at Place::Files => [
        "path" Text, "deps" Json, "test_deps" Json, "tests" Json, "calls" Json, "cites" Json, "value_uses" Json,
        "member_reads" Json, "call_paths" Json, "other_call_paths" Json, "brought" Json, "reexports" Json, "unbound_heads" Json
    ],
    "graph" at Place::One => ["nodes" Int ["graph", "nodes"], "edges" Int ["graph", "edges"]],
    "fan_in" at list(&["graph", "top_fan_in"]) => ["module" Text, "degree" Int]
}, retired ["layers", "touchpoints"]);

/// A história do git, a da branch de partida: o nome dela e, sem história, o
/// motivo; os caminhos numa tabela, em ordem, e os commits apontando para
/// ela, cada um com o título e o número do pull request que o trouxe.
pub const HISTORY: MapBlock = block!("history", version 3, {
    "history_base" at Place::One => ["base" Text ["history", "base"], "missing" Text ["history", "missing"]],
    "history_paths" at Place::List { at: &["history", "paths"], keep: false } => ["path" Text []],
    "commits" at Place::List { at: &["history", "commits"], keep: false } => [
        "id" Text, "at" Int, "title" Text, "pr" Int, "added" Json, "changed" Json
    ]
});

/// A história de cada declaração, lida do git na primeira pergunta sobre um
/// arquivo e guardada por arquivo: a branch de partida, o commit mais novo do
/// arquivo nela quando se leu, a marca do scan que leu, quantas mudanças de
/// arquivo a leitura podia seguir e quantos comentários de revisão presos ao
/// arquivo o mapa tinha; os commits lidos,
/// com o título, o número do pull request e os arquivos que cada um criou e
/// mudou; e, de cada declaração, pelo nome
/// e pela ordem entre as de mesmo nome, os commits que a mudaram, cada um
/// com a marca de só forma, e os comentários de revisão presos às linhas
/// dela. A montagem não o grava nem o confere: ele fica
/// fora de [`BLOCKS`], volta vazio na troca de versão, e a pergunta seguinte
/// o enche de novo.
pub const LINEAGE: MapBlock = block!("lineage", version 4, {
    "lineage_files" at list(&["files"]) => [
        "path" Text, "base" Text, "last_commit" Text, "tip" Text, "mark" Text, "moves" Int, "comments" Int
    ],
    "lineage_commits" at list(&["commits"]) => ["path" Text, "id" Text, "at" Int, "title" Text, "pr" Int, "files" Json],
    "lineage_decls" at list(&["declarations"]) => ["path" Text, "name" Text, "nth" Int, "commits" Json, "comments" Json]
});

/// O que o servidor disse dos pull requests da base, lido uma vez depois que
/// o commit entra nela: o título, a descrição, a marca de versão e o commit
/// mais novo da base que citava cada um; os comentários de revisão presos a
/// linhas, com o commit comentado, o arquivo e a linha; e o número de cada
/// commit que não o diz no título, 0 quando o provedor não achou. Não se
/// refaz do código nem do git: fica fora de [`BLOCKS`] e, na troca de
/// versão, é convertido, nunca apagado.
pub const PULLS: MapBlock = written(block!("pulls", version 1, {
    "pr_texts" at list(&["texts"]) => ["number" Int, "title" Text, "body" Text, "etag" Text, "through" Text],
    "pr_comments" at list(&["comments"]) => ["number" Int, "sha" Text, "path" Text, "line" Int, "body" Text],
    "pr_commits" at list(&["commits"]) => ["id" Text, "pr" Int]
}));

/// O glossário do mapa: a palavra da pergunta ligada ao nome da declaração
/// que o Claude editou logo depois de buscá-la. De cada sessão, a última
/// busca — as palavras, com as formas da normalização da busca, os lugares
/// que ela entregou, se a primeira edição ainda ensina, depois de uma busca
/// sem resultado, e a hora —; e cada marca: as formas da palavra, o arquivo
/// e o nome da declaração. Quem grava é `io::map_glossary`. Não se refaz do
/// código: fica fora de [`BLOCKS`] e, na troca de versão, é convertido,
/// nunca apagado.
pub const GLOSSARY: MapBlock = written(block!("glossary", version 1, {
    "glossary_asks" at list(&["asks"]) => ["session" Text, "words" Json, "found" Json, "first_edit" Flag, "at" Int],
    "glossary_marks" at list(&["marks"]) => ["forms" Json, "file" Text, "name" Text]
}));

/// As notas de sentido: de cada arquivo ou declaração, uma frase curta em
/// palavras de negócio, escrita pelo agente que leu o trecho — o arquivo, o
/// nome da declaração (vazio na nota do arquivo inteiro), o texto, a spec que
/// a escreveu e o blob do arquivo no momento em que foi escrita. A nota vale
/// enquanto o blob do arquivo no mapa for o dela; arquivo mudado a deixa
/// velha, e quem lê o trecho a reescreve. Quem grava e lê é `io::map_notes`.
/// Não se refaz do código: fica fora de [`BLOCKS`] e, na troca de versão, é
/// convertido, nunca apagado.
pub const NOTES: MapBlock = written(block!("notes", version 1, {
    "notes" at list(&["notes"]) => ["file" Text, "name" Text, "text" Text, "spec" Text, "blob" Text]
}));

/// O bloco declarado por [`block!`] como escrito: convertido na troca de
/// versão, nunca apagado.
const fn written(mut declared: MapBlock) -> MapBlock {
    declared.block.kind = Kind::Written(kept_as_is);
    declared
}

/// A conversão dos blocos escritos: a primeira versão não tem de onde
/// converter, e as linhas ficam como estão.
#[allow(clippy::unnecessary_wraps)] // a assinatura é a de todo bloco convertido
fn kept_as_is(_: &Connection, _: u32) -> Result<()> {
    Ok(())
}

/// Os itens das specs do projeto, com o que os liga ao código: de cada
/// item vigente de decisão, regra, pedido, tarefa, limite, contrato, erro,
/// caso de borda e fora do escopo, a spec, o número, o código, o tipo, o
/// título, a parte do usuário, a parte do agente, as palavras de busca que a
/// gravação calculou e os arquivos que ele cita ou que as tarefas dele
/// mudam; de cada commit de onda, os itens que ele cumpriu e os arquivos
/// dele; de cada spec, os números dos pull requests dela, que ligam os
/// commits da base que o squash ou o rebase criou; e, de cada spec, o último
/// número lido e o tamanho e a hora do arquivo quando se leu. Junto mora o
/// índice da busca dos itens, com o título, a parte do usuário e as palavras
/// como campos próprios. Quem o enche é `io::map_specs`; a montagem não o
/// grava nem o confere.
pub const SPECS: MapBlock = rebuilt_by(block!("specs", version 2, {
    "spec_items" at list(&["items"]) => [
        "spec" Text, "id" Int, "code" Text, "kind" Text, "title" Text, "text" Text, "agent" Text, "search" Text,
        "files" Json
    ],
    "spec_commits" at list(&["commits"]) => ["spec" Text, "sha" Text, "items" Json, "files" Json],
    "spec_pulls" at list(&["pulls"]) => ["spec" Text, "pr" Int],
    "spec_marks" at list(&["marks"]) => ["spec" Text, "last_id" Int, "size" Int, "modified" Int]
}, index [
    "spec_vocab", "spec_fts", "spec_lengths", "spec_meta"
] "CREATE INDEX spec_items_by_spec ON spec_items(spec, id);\
   CREATE INDEX spec_commits_by_sha ON spec_commits(substr(sha, 1, 10));\
   CREATE VIRTUAL TABLE spec_fts USING fts5(title, text, words, content='', contentless_delete=1, \
     tokenize='unicode61 remove_diacritics 2');\
   CREATE VIRTUAL TABLE spec_vocab USING fts5vocab(spec_fts, instance);\
   CREATE TABLE spec_lengths(id INTEGER PRIMARY KEY, title INTEGER, text INTEGER, words INTEGER);\
   CREATE TABLE spec_meta(key TEXT PRIMARY KEY, value);"), crate::io::map_specs::rebuild);

/// O bloco declarado por [`block!`] refeito por `rebuild`, e não vazio para
/// o scan encher.
const fn rebuilt_by(mut declared: MapBlock, rebuild: crate::io::map_db::Rebuild) -> MapBlock {
    declared.block.kind = Kind::Rebuilt(rebuild);
    declared
}

/// Os blocos do mapa, na ordem em que se leem: os arquivos antes das
/// declarações, das rotas e das ligações deles.
pub const BLOCKS: [MapBlock; 6] = [CENSUS, FILES, DECLS, ROUTES, GRAPH, HISTORY];

/// Os blocos de que o índice de busca lê: os arquivos, as declarações e as
/// ligações, onde moram as chamadas.
pub(crate) const SEARCHED: [&MapBlock; 3] = [&FILES, &DECLS, &GRAPH];

/// Os blocos de que o índice de busca se faz: os lidos por ela e a história
/// da base, de onde saem os títulos dos commits do arquivo. Quando algum
/// deles muda, o índice se refaz.
const INDEXED_FROM: [&MapBlock; 4] = [&FILES, &DECLS, &GRAPH, &HISTORY];

/// Todo bloco que a porta declara, na ordem do despejo: os da montagem e,
/// depois deles, o da história de cada declaração, o dos pull requests, o
/// das specs, o do glossário e o das notas de sentido.
const DECLARED: [&MapBlock; 11] =
    [&CENSUS, &FILES, &DECLS, &ROUTES, &GRAPH, &HISTORY, &LINEAGE, &PULLS, &SPECS, &GLOSSARY, &NOTES];

/// Os mesmos blocos, como o banco os abre.
const DB_BLOCKS: [Block; 11] = [
    CENSUS.block,
    FILES.block,
    DECLS.block,
    ROUTES.block,
    GRAPH.block,
    HISTORY.block,
    LINEAGE.block,
    PULLS.block,
    SPECS.block,
    GLOSSARY.block,
    NOTES.block,
];

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
    let db = open_existing(model)?;
    let json = map_text(db.conn(), every_column).map_err(unreadable)?;
    let mut map: ProjectMap =
        serde_json::from_str(&json).map_err(|e| MapRefusal::MapUnreadable { detail: e.to_string() })?;
    map.lineage = lineages(db.conn(), None).map_err(unreadable)?;
    map.census_mark = db.mark(CENSUS.name()).map_err(unreadable)?.unwrap_or_default();
    map.pulls = every_pull(db.conn()).map_err(unreadable)?;
    map.spec_notes = crate::io::map_specs::notes_of(db.conn(), None).map_err(unreadable)?;
    Ok(map)
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
    stored_part(model, every_column)
}

/// O estado da leitura gravado em `model`, com as mesmas recusas de
/// [`read`]: o censo inteiro, de cada arquivo só o caminho, o blob e os
/// sinais de código, e a marca de cada bloco. É o que a passada do scan
/// consulta para saber se tem arquivo a reler, sem as declarações, o grafo e
/// a história, que ficam no banco. A coluna estragada que ela não lê não a
/// recusa.
pub fn read_state_at(model: &Path) -> std::result::Result<StoredMap, MapRefusal> {
    stored_part(model, state_column)
}

/// Quais colunas de cada tabela uma leitura do mapa em JSON traz.
type Pick = fn(&Table, &Column) -> bool;

/// Toda coluna de toda tabela: o mapa inteiro.
fn every_column(_: &Table, _: &Column) -> bool {
    true
}

/// As colunas dos arquivos que o estado da leitura traz: o caminho, que
/// identifica o arquivo, o blob, que diz se ele mudou, e os sinais de código,
/// que as pilhas contam.
const STATE_FILE_COLUMNS: [&str; 3] = ["path", "blob", "signals"];

/// O estado da leitura: toda tabela do censo e, na dos arquivos, as colunas
/// de [`STATE_FILE_COLUMNS`].
fn state_column(table: &Table, column: &Column) -> bool {
    CENSUS.tables.iter().any(|census| census.name == table.name)
        || (FILES.tables.iter().any(|files| files.name == table.name) && STATE_FILE_COLUMNS.contains(&column.name))
}

/// O mapa gravado em `model` com só as colunas que `pick` escolhe, e a marca
/// de cada bloco.
fn stored_part(model: &Path, pick: Pick) -> std::result::Result<StoredMap, MapRefusal> {
    let db = open_existing(model)?;
    let json = map_text(db.conn(), pick).map_err(unreadable)?;
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
/// coluna vazia como `null` e o texto JSON já lido. O índice de busca fica
/// de fora, com as tabelas virtuais e as que o FTS5 guarda por trás delas:
/// ele se refaz do mapa e não se lê de volta.
pub fn dump(root: &Path) -> std::result::Result<Value, MapRefusal> {
    let db = open_existing(&model_path(root))?;
    dump_tables(db.conn()).map_err(unreadable)
}

fn dump_tables(conn: &Connection) -> Result<Value> {
    let mut out = Vec::new();
    for table in DECLARED.iter().flat_map(|block| block.tables) {
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
    let declared: Vec<&str> = DB_BLOCKS.iter().flat_map(|block| block.tables.iter().copied()).collect();
    let mut stmt = conn.prepare(
        "SELECT name FROM pragma_table_list WHERE schema = 'main' AND type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
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
    /// O resumo do mapa: o caminho de cada arquivo, as línguas,
    /// os subprojetos, os arquivos mais importados e a história.
    Summary,
    /// O terreno: os subprojetos e a camada de cada pasta.
    Terrain,
    /// Os caminhos dos arquivos, e nada mais deles.
    Paths,
    /// Quem importa o arquivo: ele e os arquivos que o importam, com as
    /// importações de cada um.
    Importers(&'a str),
    /// Os testes do arquivo: ele, com os testes que o cobrem e a marca dos
    /// próprios testes.
    Tests(&'a str),
    /// As partes do arquivo: ele, com as linhas dos próprios testes e as
    /// declarações dele, só com o tipo, o nome e as linhas.
    Parts(&'a str),
    /// As declarações com o nome, com o arquivo delas; com `file`, só as
    /// desse arquivo, que vem mesmo sem nenhuma.
    Declarations { file: Option<&'a str>, name: &'a str },
    /// Os exemplos para uma tarefa: cada arquivo com o tamanho, a classe, os
    /// testes e as importações, e a história. Com `words`, também os nomes
    /// que cada arquivo declara, que a busca da pasta usa.
    Examples { words: bool },
    /// A história de uma declaração: as declarações com o nome, com o
    /// arquivo delas (com `file`, só as dele), a história do git, a de cada
    /// declaração desses arquivos, quando guardada, e a marca da versão do
    /// scan que gravou o censo.
    History { file: Option<&'a str>, name: &'a str },
    /// O texto do pull request com o número, quando o mapa o tem.
    Pull(u32),
    /// A história guardada das declarações do arquivo, quando há, com o que
    /// a validade dela confere além da história do git: a marca da versão do
    /// scan que gravou o censo e os comentários de revisão presos ao arquivo.
    Lineage(&'a str),
}

/// Como quem pergunta ao mapa o lê: por [`read_for`], só as tabelas da
/// pergunta. Um teste passa o mapa inteiro no lugar, para conferir que a
/// resposta é a mesma.
pub type MapReader<'r> = dyn Fn(Need<'_>) -> std::result::Result<ProjectMap, MapRefusal> + 'r;

/// O mapa do projeto em `root` com só o que `need` pede, com as mesmas
/// recusas de [`read`] e, quando um bloco que a pergunta lê voltou vazio
/// numa troca de formato sem que o scan o enchesse de novo,
/// [`MapRefusal::MapUnfilled`].
pub fn read_for(root: &Path, need: Need<'_>) -> std::result::Result<ProjectMap, MapRefusal> {
    read_for_at(&model_path(root), need)
}

/// O mapa gravado em `model` com só o que `need` pede, como [`read_for`]: é
/// a leitura do mapa que o scan gravou num caminho escolhido por quem o
/// chamou.
pub fn read_for_at(model: &Path, need: Need<'_>) -> std::result::Result<ProjectMap, MapRefusal> {
    let db = open_existing(model)?;
    map_fill::refuse(&db, map_fill::read_by(need))?;
    part_of(&db, need).map_err(unreadable)
}

/// O blob do git do conteúdo de cada arquivo de `paths` como o mapa em
/// `model` o leu, pelo caminho: com ele se vê se o arquivo mudou depois da
/// passada do scan. O arquivo que o mapa não guarda fica de fora. Lê só as
/// linhas dos arquivos pedidos, com as mesmas recusas de [`read`].
pub fn blobs_of(model: &Path, paths: &[&str]) -> std::result::Result<BTreeMap<String, String>, MapRefusal> {
    let db = open_existing(model)?;
    let mut blobs = BTreeMap::new();
    for path in paths {
        let rows = file_rows(db.conn(), &["path", "blob"], Some(&crate::domain::project_map::clean_path(path)))
            .map_err(unreadable)?;
        if let Some(row) = rows.first() {
            blobs.insert(text_cell(&row[0]), text_cell(&row[1]));
        }
    }
    Ok(blobs)
}

/// A história do git guardada no mapa em `model`, sem ler outra tabela, com
/// as mesmas recusas de [`read`].
pub fn history_at(model: &Path) -> std::result::Result<History, MapRefusal> {
    let db = open_existing(model)?;
    history(db.conn()).map_err(unreadable)
}

/// Os subprojetos do mapa gravado em `model`, com todas as colunas da tabela
/// deles, na ordem do mapa, sem ler outra tabela. Com as mesmas recusas de
/// [`read`].
pub fn projects_at(model: &Path) -> std::result::Result<Vec<crate::domain::scan::Project>, MapRefusal> {
    let db = open_existing(model)?;
    let table = declared_table("projects").map_err(unreadable)?;
    let rows = rows_in(db.conn(), table).map_err(unreadable)?;
    rows.iter()
        .map(|row| {
            let mut object = Map::new();
            for (column, value) in table.columns.iter().zip(row) {
                if let Some(value) = json_of(table, column, value)? {
                    object.insert(column.name.to_string(), value);
                }
            }
            serde_json::from_value(Value::Object(object)).map_err(|e| e.to_string())
        })
        .collect::<std::result::Result<Vec<_>, String>>()
        .map_err(|detail| MapRefusal::MapUnreadable { detail })
}

/// O que `need` lê do banco `db`, sem conferir os blocos que o scan ainda
/// tem de encher.
pub(crate) fn part_of(db: &MapDb, need: Need<'_>) -> Result<ProjectMap> {
    use crate::domain::project_map::clean_path;
    let conn = db.conn();
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
        Need::Parts(file) => map.modules = parts_of(conn, &clean_path(file))?,
        Need::Declarations { file, name } => {
            map.modules = named(conn, file.map(clean_path).as_deref(), name.trim())?;
        }
        Need::Examples { words } => {
            map.modules = example_modules(conn, words)?;
            map.history = history(conn)?;
        }
        Need::History { file, name } => {
            map.modules = named(conn, file.map(clean_path).as_deref(), name.trim())?;
            map.history = history(conn)?;
            let paths: Vec<&str> = map.modules.iter().map(|module| module.path.as_str()).collect();
            map.lineage = lineages(conn, Some(&paths))?;
            map.census_mark = db.mark(CENSUS.name())?.unwrap_or_default();
            map.pulls = pulls_of(conn, &paths, &map.lineage)?;
            let ids: Vec<&str> =
                map.lineage.iter().flat_map(|lineage| lineage.commits.iter().map(|commit| commit.id.as_str())).collect();
            map.spec_notes = crate::io::map_specs::notes_of(conn, Some(&ids))?;
        }
        Need::Lineage(file) => {
            let file = clean_path(file);
            map.lineage = lineages(conn, Some(&[file.as_str()]))?;
            map.census_mark = db.mark(CENSUS.name())?.unwrap_or_default();
            map.pulls.comments = pulls_of(conn, &[file.as_str()], &[])?.comments;
        }
        Need::Pull(number) => {
            let number = number.to_string();
            map.pulls.texts = pull_texts(conn, &format!("{} = ?1", column_names("pr_texts", &["number"])?), &[&number])?;
        }
    }
    Ok(map)
}

/// Tudo o que o bloco dos pull requests guarda.
fn every_pull(conn: &Connection) -> Result<Pulls> {
    Ok(Pulls {
        texts: pull_texts(conn, "", &[])?,
        comments: picked(conn, "pr_comments", &["number", "sha", "path", "line", "body"], "", &[])?
            .iter()
            .map(pull_comment)
            .collect(),
        commits: picked(conn, "pr_commits", &["id", "pr"], "", &[])?.iter().map(pull_of_commit).collect(),
    })
}

fn pull_comment(row: &Picked) -> PullComment {
    PullComment {
        number: u32::try_from(int_cell(&row[0])).unwrap_or_default(),
        commit: text_cell(&row[1]),
        path: text_cell(&row[2]),
        line: u64::try_from(int_cell(&row[3])).unwrap_or_default(),
        body: text_cell(&row[4]),
    }
}

fn pull_of_commit(row: &Picked) -> PullOfCommit {
    PullOfCommit { id: text_cell(&row[0]), pr: u32::try_from(int_cell(&row[1])).unwrap_or_default() }
}

/// O que o bloco dos pull requests guarda para a história dos arquivos
/// `paths`: os comentários presos a eles, o número de cada commit das listas
/// `lineage` que o provedor achou, e o texto dos pull requests desses
/// commits.
fn pulls_of(conn: &Connection, paths: &[&str], lineage: &[FileLineage]) -> Result<Pulls> {
    let within = |table: &str, column: &str, values: &[&str]| -> Result<String> {
        let slots: Vec<String> = (1..=values.len()).map(|at| format!("?{at}")).collect();
        Ok(format!("{} IN ({})", column_names(table, &[column])?, slots.join(", ")))
    };
    let mut pulls = Pulls::default();
    if !paths.is_empty() {
        let filter = within("pr_comments", "path", paths)?;
        pulls.comments = picked(conn, "pr_comments", &["number", "sha", "path", "line", "body"], &filter, paths)?
            .iter()
            .map(pull_comment)
            .collect();
    }
    let ids: Vec<&str> = lineage.iter().flat_map(|file| file.commits.iter().map(|commit| commit.id.as_str())).collect();
    if !ids.is_empty() {
        pulls.commits =
            picked(conn, "pr_commits", &["id", "pr"], &within("pr_commits", "id", &ids)?, &ids)?.iter().map(pull_of_commit).collect();
    }
    let numbers: BTreeSet<String> = lineage
        .iter()
        .flat_map(|file| file.commits.iter().filter_map(|commit| commit.pr))
        .chain(pulls.commits.iter().map(|commit| commit.pr).filter(|pr| *pr > 0))
        .map(|pr| pr.to_string())
        .collect();
    let numbers: Vec<&str> = numbers.iter().map(String::as_str).collect();
    if !numbers.is_empty() {
        let filter = within("pr_texts", "number", &numbers)?;
        pulls.texts = pull_texts(conn, &filter, &numbers)?;
    }
    Ok(pulls)
}

/// O texto dos pull requests que `filter` deixa, com a marca de versão e o
/// commit até onde cada um foi lido.
fn pull_texts(conn: &Connection, filter: &str, params: &[&str]) -> Result<Vec<PullText>> {
    Ok(picked(conn, "pr_texts", &["number", "title", "body", "etag", "through"], filter, params)?
        .iter()
        .map(|row| PullText {
            number: u32::try_from(int_cell(&row[0])).unwrap_or_default(),
            title: text_cell(&row[1]),
            body: text_cell(&row[2]),
            etag: text_cell(&row[3]),
            through: text_cell(&row[4]),
        })
        .collect())
}

/// De onde parte a leitura dos pull requests: os commits da história
/// guardada da base, os das listas por arquivo, e o que o bloco dos pull
/// requests já tem — os textos sem a descrição, e o número de cada commit
/// já perguntado.
#[derive(Debug, Clone, Default)]
pub struct PullSources {
    pub window: Vec<Commit>,
    pub lineage: Vec<LineageCommit>,
    pub texts: Vec<PullText>,
    pub asked: Vec<PullOfCommit>,
}

/// O que a leitura dos pull requests precisa do mapa em `model`, como
/// [`PullSources`] o descreve. Com as recusas de [`read`].
pub fn pull_sources_at(model: &Path) -> std::result::Result<PullSources, MapRefusal> {
    let db = open_existing(model)?;
    let conn = db.conn();
    let read = || -> Result<PullSources> {
        let lineage = picked(conn, "lineage_commits", &["id", "at", "title", "pr"], "", &[])?
            .iter()
            .map(|row| LineageCommit {
                id: text_cell(&row[0]),
                at: int_cell(&row[1]),
                title: text_cell(&row[2]),
                pr: u32::try_from(int_cell(&row[3])).ok().filter(|n| *n > 0),
                ..LineageCommit::default()
            })
            .collect();
        let texts = picked(conn, "pr_texts", &["number", "etag", "through"], "", &[])?
            .iter()
            .map(|row| PullText {
                number: u32::try_from(int_cell(&row[0])).unwrap_or_default(),
                etag: text_cell(&row[1]),
                through: text_cell(&row[2]),
                ..PullText::default()
            })
            .collect();
        let asked = picked(conn, "pr_commits", &["id", "pr"], "", &[])?.iter().map(pull_of_commit).collect();
        Ok(PullSources { window: history(conn)?.commits, lineage, texts, asked })
    };
    read().map_err(unreadable)
}

/// Os comentários de revisão presos ao arquivo `path` no mapa em `model`,
/// na ordem em que se gravaram. Com as recusas de [`read`].
pub fn pull_comments_at(model: &Path, path: &str) -> std::result::Result<Vec<PullComment>, MapRefusal> {
    let db = open_existing(model)?;
    Ok(pulls_of(db.conn(), &[path], &[]).map_err(unreadable)?.comments)
}

/// Os comentários de revisão presos a cada arquivo de `paths` no mapa em
/// `model`, todos numa leitura só, na ordem em que se gravaram. Com as
/// recusas de [`read`].
pub fn pull_comments_for_at(model: &Path, paths: &[&str]) -> std::result::Result<Vec<PullComment>, MapRefusal> {
    let db = open_existing(model)?;
    Ok(pulls_of(db.conn(), paths, &[]).map_err(unreadable)?.comments)
}

/// Grava no mapa em `model` o texto do pull request e os comentários presos
/// a linhas dele, no lugar do que o mapa tinha desse número.
///
/// # Errors
///
/// Sem o mapa, com ele ilegível ou quando a gravação falha.
pub fn save_pull_at(model: &Path, text: &PullText, comments: &[PullComment]) -> Result<()> {
    let number = Sql::Integer(i64::from(text.number));
    let texts = vec![vec![
        number.clone(),
        Sql::Text(text.title.clone()),
        Sql::Text(text.body.clone()),
        Sql::Text(text.etag.clone()),
        Sql::Text(text.through.clone()),
    ]];
    let comments = comments
        .iter()
        .map(|comment| {
            vec![
                number.clone(),
                Sql::Text(comment.commit.clone()),
                Sql::Text(comment.path.clone()),
                Sql::Integer(i64::try_from(comment.line).unwrap_or(i64::MAX)),
                Sql::Text(comment.body.clone()),
            ]
        })
        .collect();
    replace_rows(model, vec![("pr_texts", "number", number.clone(), texts), ("pr_comments", "number", number, comments)])
}

/// Marca no mapa em `model` que o texto do pull request `number` segue o
/// mesmo até o commit `through`, sem regravar o texto.
///
/// # Errors
///
/// Sem o mapa, com ele ilegível ou quando a gravação falha.
pub fn keep_pull_at(model: &Path, number: u32, through: &str) -> Result<()> {
    let mut db = open_existing(model).map_err(|refusal| Error::Parse(format!("{refusal:?}")))?;
    db.write(|tx| {
        tx.execute(
            &format!("UPDATE {} SET {} = ?1 WHERE {} = ?2", table_name("pr_texts")?, quoted("through"), quoted("number")),
            rusqlite::params![through, i64::from(number)],
        )?;
        Ok(())
    })
}

/// Grava no mapa em `model` o número que o provedor deu a cada commit sem
/// número no título, no lugar do que o mapa tinha desses commits.
///
/// # Errors
///
/// Sem o mapa, com ele ilegível ou quando a gravação falha.
pub fn save_pull_commits_at(model: &Path, found: &[PullOfCommit]) -> Result<()> {
    let changes = found
        .iter()
        .map(|commit| {
            let id = Sql::Text(commit.id.clone());
            ("pr_commits", "id", id.clone(), vec![vec![id, Sql::Integer(i64::from(commit.pr))]])
        })
        .collect();
    replace_rows(model, changes)
}

/// Troca, numa transação só, as linhas de cada tabela em que a coluna dada
/// vale o valor dado pelas linhas novas, na ordem das colunas declaradas.
fn replace_rows(model: &Path, changes: Vec<(&str, &str, Sql, Vec<Row>)>) -> Result<()> {
    let mut db = open_existing(model).map_err(|refusal| Error::Parse(format!("{refusal:?}")))?;
    db.write(|tx| {
        replace_in(tx, &changes)?;
        map_revision::bump(tx)?;
        Ok(())
    })
}

/// A troca de [`replace_rows`] dentro da transação de quem grava.
fn replace_in(tx: &Connection, changes: &[Replacement<'_>]) -> Result<()> {
    for (table, key, value, rows) in changes {
        let found = declared_table(table)?;
        tx.execute(&format!("DELETE FROM {} WHERE {} = ?1", quoted(found.name), quoted(key)), [value])?;
        let names: Vec<String> = found.columns.iter().map(|column| quoted(column.name)).collect();
        let slots = vec!["?"; names.len()].join(", ");
        let mut insert = tx.prepare(&format!("INSERT INTO {}({}) VALUES ({slots})", quoted(found.name), names.join(", ")))?;
        for row in rows {
            insert.execute(params_from_iter(row))?;
        }
    }
    Ok(())
}

/// O nome entre aspas de uma tabela declarada; a tabela que nenhum bloco
/// declara é recusada.
fn table_name(table: &str) -> Result<String> {
    declared_table(table).map(|found| quoted(found.name))
}

fn declared_table(table: &str) -> Result<&'static Table> {
    DECLARED
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
    let commits = picked(conn, "commits", &["id", "at", "title", "pr", "added", "changed"], "", &[])?
        .iter()
        .map(|row| {
            Ok(Commit {
                id: text_cell(&row[0]),
                at: int_cell(&row[1]),
                title: text_cell(&row[2]),
                pr: u32::try_from(int_cell(&row[3])).ok().filter(|n| *n > 0),
                added: json_cell(&row[4])?,
                changed: json_cell(&row[5])?,
            })
        })
        .collect::<Result<_>>()?;
    let (base, missing) = match picked(conn, "history_base", &["base", "missing"], "", &[])?.first() {
        Some(row) => (text_cell(&row[0]), serde_json::from_value(Value::String(text_cell(&row[1]))).ok()),
        None => (String::new(), None),
    };
    Ok(History { base, missing, paths, commits })
}

/// As colunas `columns` das linhas da tabela `table` da história guardada,
/// na ordem em que se gravaram: de todos os arquivos, ou só dos caminhos
/// `paths`.
fn lineage_rows(conn: &Connection, table: &str, columns: &[&str], paths: Option<&[&str]>) -> Result<Vec<Picked>> {
    match paths {
        None => picked(conn, table, columns, "", &[]),
        Some([]) => Ok(Vec::new()),
        Some(paths) => {
            let slots: Vec<String> = (1..=paths.len()).map(|at| format!("?{at}")).collect();
            let filter = format!("{} IN ({})", column_names(table, &["path"])?, slots.join(", "));
            picked(conn, table, columns, &filter, paths)
        }
    }
}

/// O cabeçalho da história guardada de cada arquivo, na ordem em que se
/// gravou: a base, o commit mais novo, a marca do scan e as contagens que
/// dizem se ela ainda vale, sem os commits nem as declarações; com `paths`,
/// só o desses arquivos. A pergunta da história e a busca o leem por aqui,
/// e por isso conferem a validade pelos mesmos valores.
pub(crate) fn lineage_heads(conn: &Connection, paths: Option<&[&str]>) -> Result<Vec<FileLineage>> {
    let columns = ["path", "base", "last_commit", "tip", "mark", "moves", "comments"];
    Ok(lineage_rows(conn, "lineage_files", &columns, paths)?
        .iter()
        .map(|row| FileLineage {
            path: text_cell(&row[0]),
            base: text_cell(&row[1]),
            last_commit: text_cell(&row[2]),
            tip: text_cell(&row[3]),
            mark: text_cell(&row[4]),
            moves: u32::try_from(int_cell(&row[5])).unwrap_or_default(),
            comments: u32::try_from(int_cell(&row[6])).unwrap_or_default(),
            ..FileLineage::default()
        })
        .collect())
}

/// A história guardada das declarações de cada arquivo, na ordem em que se
/// gravou; com `paths`, só a desses arquivos.
pub(crate) fn lineages(conn: &Connection, paths: Option<&[&str]>) -> Result<Vec<FileLineage>> {
    let mut files = lineage_heads(conn, paths)?;
    let at: HashMap<String, usize> = files.iter().enumerate().map(|(at, file)| (file.path.clone(), at)).collect();
    for row in lineage_rows(conn, "lineage_commits", &["path", "id", "at", "title", "pr", "files"], paths)? {
        if let Some(&file) = at.get(&text_cell(&row[0])) {
            files[file].commits.push(LineageCommit {
                id: text_cell(&row[1]),
                at: int_cell(&row[2]),
                title: text_cell(&row[3]),
                pr: u32::try_from(int_cell(&row[4])).ok().filter(|n| *n > 0),
                files: json_cell(&row[5])?,
            });
        }
    }
    for row in lineage_rows(conn, "lineage_decls", &["path", "name", "nth", "commits", "comments"], paths)? {
        if let Some(&file) = at.get(&text_cell(&row[0])) {
            files[file].declarations.push(DeclLineage {
                name: text_cell(&row[1]),
                nth: u32::try_from(int_cell(&row[2])).unwrap_or_default(),
                commits: json_cell(&row[3])?,
                comments: json_cell(&row[4])?,
            });
        }
    }
    Ok(files)
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

/// O arquivo `file`, com as linhas dos próprios testes e as declarações dele,
/// só com o tipo, o nome e as linhas, na ordem do mapa. O arquivo que o mapa
/// não tem não vem.
fn parts_of(conn: &Connection, file: &str) -> Result<Vec<MapModule>> {
    let Some(row) = file_rows(conn, &["path", "test_lines"], Some(file))?.into_iter().next() else { return Ok(Vec::new()) };
    let filter = format!("{} = ?1", column_names("decls", &["file"])?);
    let declarations = picked(conn, "decls", &["kind", "name", "line", "end_line"], &filter, &[file])?
        .iter()
        .map(|row| MapDecl {
            kind: text_cell(&row[0]),
            name: text_cell(&row[1]),
            line: int_cell(&row[2]) as u64,
            end_line: int_cell(&row[3]) as u64,
            ..MapDecl::default()
        })
        .collect();
    Ok(vec![MapModule { path: text_cell(&row[0]), test_lines: json_cell(&row[1])?, declarations, ..MapModule::default() }])
}

/// As colunas de uma declaração que as perguntas pelo nome leem.
const NAMED_COLUMNS: [&str; 14] = [
    "file", "kind", "name", "line", "end_line", "doc", "signature", "used_by",
    "owner", "contract", "members", "implements", "implemented_by", "common_calls",
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
        common_calls: usize::try_from(int_cell(&row[13])).unwrap_or_default(),
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
        with_routes(conn, &mut modules)?;
        return Ok(modules);
    };
    let Some(row) = file_rows(conn, &["path"], Some(file))?.into_iter().next() else { return Ok(Vec::new()) };
    let mut module = module_of(row)?;
    for row in picked(conn, "decls", &NAMED_COLUMNS, &format!("{decl_file} = ?1 AND {decl_name} = ?2"), &[file, name])? {
        module.declarations.push(named_decl(&row)?);
    }
    let mut modules = vec![module];
    with_routes(conn, &mut modules)?;
    Ok(modules)
}

/// As rotas de cada arquivo de `modules` que declara alguma coisa: as que a
/// declaração atende, com as chamadas da tela que as alcançam.
fn with_routes(conn: &Connection, modules: &mut [MapModule]) -> Result<()> {
    let path = column_names("routes", &["path"])?;
    for module in modules.iter_mut().filter(|module| !module.declarations.is_empty()) {
        if let Some(row) = picked(conn, "routes", &["routes"], &format!("{path} = ?1"), &[module.path.as_str()])?.first() {
            module.routes = json_cell(&row[0])?;
        }
    }
    Ok(())
}

/// Cada arquivo com o que os exemplos leem dele: o tamanho, a classe, a
/// marca dos próprios testes, as importações, os testes que o cobrem e as
/// medidas de qualidade; com `words`, também os nomes que ele declara.
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
    for row in picked(conn, "texts", &["path", "quality"], "", &[])? {
        if let Some(&index) = at.get(&text_cell(&row[0])) {
            modules[index].quality = json_cell(&row[1])?;
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
    /// Os caminhos que o índice do git guarda, com os dos submódulos
    /// iniciados: só o que foi adicionado ao git, sem o arquivo novo que
    /// ninguém adicionou.
    pub indexed: BTreeSet<String>,
    /// A branch de partida do projeto e o commit da ponta dela.
    pub base: Base,
}

/// A branch de partida que o projeto declara no `mustard.json` e o commit da
/// ponta dela, de onde vem a história do git que o mapa guarda.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Base {
    /// O nome declarado; vazio quando o projeto não declara nenhum.
    pub name: String,
    /// O commit da ponta: a do servidor (`origin/<nome>`) quando o clone a
    /// tem, senão a local; vazio quando nenhuma das duas existe.
    pub tip: String,
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
    let own = [MAP_FILE_NAME, MAP_JOURNAL_FILE_NAME, MAP_WAL_FILE_NAME, MAP_SHARED_FILE_NAME]
        .map(|name| format!("{MAP_DIR}/{name}"));
    let (blobs, indexed) = blobs_under(root, &own)?;
    let head = git_out(root, &["rev-parse", "--verify", "-q", "HEAD"]).map(|out| out.trim().to_string()).unwrap_or_default();
    Some(Listing { head, blobs, indexed, base: base_of(root) })
}

/// A branch de partida do projeto em `root`, pela configuração dele, com a
/// ponta que o clone tem: a do servidor antes da local, numa chamada só ao
/// git. O projeto que não declara nenhuma parte da branch padrão do servidor
/// (`refs/remotes/origin/HEAD`) e, sem servidor, da branch em que o checkout
/// está; só se lê o git, nada se grava. Sem declaração, sem servidor e com o
/// checkout solto de branch, não há base.
#[must_use]
pub fn base_of(root: &Path) -> Base {
    let Some(name) = crate::domain::config::ProjectConfig::load(root).git.primary_base().or_else(|| default_branch(root)) else {
        return Base::default();
    };
    let (remote, local) = (format!("refs/remotes/origin/{name}"), format!("refs/heads/{name}"));
    let refs = git_out(root, &["for-each-ref", "--format=%(objectname) %(refname)", &remote, &local]).unwrap_or_default();
    let tip_of = |wanted: &str| {
        refs.lines().find_map(|line| line.split_once(' ').filter(|(_, name)| *name == wanted).map(|(tip, _)| tip.to_string()))
    };
    let tip = tip_of(&remote).or_else(|| tip_of(&local)).unwrap_or_default();
    Base { name, tip }
}

/// A branch de partida de um projeto que não declara nenhuma: a que o
/// servidor aponta como padrão, ou, sem servidor, a do checkout. `None` com o
/// checkout solto de branch.
fn default_branch(root: &Path) -> Option<String> {
    let of = |args: &[&str], prefix: &str| {
        let out = git_out(root, args)?;
        out.trim().strip_prefix(prefix).filter(|name| !name.is_empty()).map(str::to_string)
    };
    of(&["symbolic-ref", "-q", "refs/remotes/origin/HEAD"], "refs/remotes/origin/")
        .or_else(|| of(&["symbolic-ref", "-q", "HEAD"], "refs/heads/"))
}

/// O blob de cada arquivo sob `root`, com os de dentro dos submódulos
/// iniciados, pelo caminho relativo a `root`, fora os caminhos `skip`, que
/// nem se calculam; e os caminhos que o índice do git guarda, da mesma
/// leitura do índice.
fn blobs_under(root: &Path, skip: &[String]) -> Option<(BTreeMap<String, String>, BTreeSet<String>)> {
    let staged = git_out(root, &["ls-files", "-s", "-z"])?;
    let mut blobs = BTreeMap::new();
    let mut indexed = BTreeSet::new();
    let mut nested = Vec::new();
    for entry in staged.split('\0') {
        let Some((meta, path)) = entry.split_once('\t') else { continue };
        let mut parts = meta.split(' ');
        let (Some(mode), Some(blob)) = (parts.next(), parts.next()) else { continue };
        if mode == SUBMODULE_MODE {
            nested.push(path.to_string());
        } else if !skip.iter().any(|own| own == path) {
            blobs.insert(path.to_string(), blob.to_string());
            indexed.insert(path.to_string());
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
        let (inner, inner_indexed) = blobs_under(&dir, &[]).unwrap_or_default();
        for (path, blob) in inner {
            blobs.insert(format!("{sub}/{path}"), blob);
        }
        indexed.extend(inner_indexed.into_iter().map(|path| format!("{sub}/{path}")));
    }
    Some((blobs, indexed))
}

/// O mapa de `root` ficou atrás do conteúdo de agora: o commit do checkout,
/// a branch de partida, a ponta dela ou algum arquivo mudou desde a passada
/// que o gravou; um bloco que ela grava voltou vazio numa troca de formato
/// ([`map_fill::unfilled`]); ou a passada foi de outro scan que o de
/// `scan_format`, a marca do que refaria o mapa ([`map_format`]), mesmo com o
/// projeto parado. `scan_format` só é chamado quando o mapa traz marca com
/// que comparar, e sem marca do scan (`None`) a compilação dele não conta:
/// quem não acha o scan não tem como refazer o mapa. Lê só o estado gravado e
/// as marcas dos blocos, nunca o mapa inteiro. Sem o arquivo do mapa, dentro
/// do git, o mapa está atrás de tudo: falta criá-lo, e quem refaz o mapa o
/// cria. `false` com um mapa que não se lê e fora do git: não há com que
/// comparar.
#[must_use]
pub fn is_behind(root: &Path, scan_format: &dyn Fn() -> Option<String>) -> bool {
    if !exists_at(&model_path(root)) {
        return inside_work_tree(root);
    }
    let Ok(db) = open_existing(&model_path(root)) else { return false };
    let Ok(rows) = picked(db.conn(), "census", &["head", "listing", "base", "base_tip"], "", &[]) else { return false };
    let Some(now) = listing(root) else { return false };
    let (head, digest, base) = rows.first().map_or_else(Default::default, |row| {
        (text_cell(&row[0]), text_cell(&row[1]), Base { name: text_cell(&row[2]), tip: text_cell(&row[3]) })
    });
    head != now.head
        || digest != now.digest()
        || base != now.base
        || map_fill::unfilled(&db, &BLOCKS).is_ok_and(|blocks| !blocks.is_empty())
        || map_format::written_by_another(&db, scan_format).unwrap_or(false)
}

/// `true` quando `root` está dentro da árvore de trabalho de um repositório
/// git, a condição para o mapa se ler do projeto.
fn inside_work_tree(root: &Path) -> bool {
    git_out(root, &["rev-parse", "--is-inside-work-tree"]).is_some_and(|out| out.trim() == "true")
}

/// O banco em `model`, que já tem de existir: sem o arquivo, a recusa de
/// mapa ausente, e nada se cria. O arquivo que não começa como um banco —
/// vazio, ou com o texto que não se entendeu — é recusado antes de abrir: o
/// SQLite tomaria o arquivo curto por um banco novo e gravaria nele.
pub(crate) fn open_existing(model: &Path) -> std::result::Result<MapDb, MapRefusal> {
    existing(model)?;
    open(model).map_err(unreadable)
}

/// [`open_existing`] com a espera `wait` pela trava de outra gravação, para
/// quem desiste logo em vez de segurar a ação de quem o chamou.
pub(crate) fn open_existing_waiting(model: &Path, wait: std::time::Duration) -> std::result::Result<MapDb, MapRefusal> {
    existing(model)?;
    MapDb::open_waiting(model, project_of(model), &DB_BLOCKS, wait).map_err(unreadable)
}

/// A recusa do mapa que falta ou que não começa como um banco.
fn existing(model: &Path) -> std::result::Result<(), MapRefusal> {
    if !exists_at(model) {
        return Err(MapRefusal::MapMissing);
    }
    if head_of(model) != SQLITE_HEADER {
        return Err(MapRefusal::MapUnreadable { detail: format!("{} is not a SQLite database", model.display()) });
    }
    Ok(())
}

/// Abre o banco em `model` com os blocos do mapa em dia.
fn open(model: &Path) -> Result<MapDb> {
    MapDb::open(model, project_of(model), &DB_BLOCKS)
}

/// A raiz do projeto do mapa em `model`. Ela só serve ao bloco refeito, e
/// todo bloco do mapa volta vazio para o scan encher: a pasta de cima basta.
fn project_of(model: &Path) -> &Path {
    model.parent().and_then(Path::parent).unwrap_or_else(|| Path::new("."))
}

pub(crate) fn unreadable(err: Error) -> MapRefusal {
    MapRefusal::MapUnreadable { detail: err.to_string() }
}

/// As linhas de uma tabela, na ordem em que entraram.
type Row = Vec<Sql>;

/// O que uma gravação troca em uma tabela: a tabela, a coluna que a chave
/// nomeia, o valor da chave e as linhas novas.
type Replacement<'a> = (&'a str, &'a str, Sql, Vec<Row>);

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
/// cada um, só com as colunas que `pick` escolhe. A coluna em JSON entra como
/// está; o texto que não se lê é recusado por quem lê o mapa. A tabela por
/// arquivo que fica traz sempre a primeira coluna, a que diz o arquivo.
fn map_text(conn: &Connection, pick: Pick) -> Result<String> {
    let mut top: Object = Vec::new();
    let mut modules: Vec<ModuleText> = Vec::new();
    let mut by_path: HashMap<Option<String>, usize> = HashMap::new();
    let mut files_table: Option<&str> = None;
    for table in BLOCKS.iter().flat_map(|block| block.tables) {
        let columns: Vec<&Column> = table.columns.iter().filter(|column| pick(table, column)).collect();
        if columns.is_empty() {
            continue;
        }
        let names: Vec<String> = columns.iter().map(|column| quoted(column.name)).collect();
        let mut stmt = conn.prepare(&format!("SELECT {} FROM {} ORDER BY rowid", names.join(", "), quoted(table.name)))?;
        let mut rows = stmt.query([])?;
        match table.place {
            Place::One => {
                while let Some(row) = rows.next()? {
                    for (at, column) in columns.iter().enumerate() {
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
                    push_item(&mut list, table, &columns, row)?;
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
                    push_fields(&mut module.fields, table, &columns, row, usize::from(!first), comma)?;
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
                    push_fields(declarations, table, &columns, row, 1, false)?;
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
/// da coluna `skip` das lidas, `columns`, separadas por vírgula; a primeira
/// também, quando `comma`. As colunas de uma linha de lista ou de arquivo
/// têm nome de um nível só.
fn push_fields(
    out: &mut String,
    table: &Table,
    columns: &[&Column],
    row: &rusqlite::Row<'_>,
    skip: usize,
    mut comma: bool,
) -> Result<()> {
    for (at, column) in columns.iter().enumerate().skip(skip) {
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

/// Escreve em `out` um item de lista, das colunas lidas `columns`: o próprio
/// valor, na tabela de uma coluna só com o caminho vazio; senão, o objeto
/// das colunas.
fn push_item(out: &mut String, table: &Table, columns: &[&Column], row: &rusqlite::Row<'_>) -> Result<()> {
    if let [column] = columns
        && column.key.is_empty()
    {
        if !push_cell(out, table, column, row.get_ref(0)?)? {
            out.push_str("null");
        }
        return Ok(());
    }
    out.push('{');
    push_fields(out, table, columns, row, 0, false)?;
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
/// banco é trocado pelo mapa. Regravados os arquivos, as declarações ou as
/// ligações, o índice de busca se refaz na mesma transação, com as palavras
/// preparadas nas línguas `languages`.
///
/// # Errors
///
/// A chave que não cabe na coluna dela ([`Error::Parse`]) e a falha do banco.
pub fn save_at(model: &Path, map: &Value, mark: &str, languages: &Languages) -> Result<bool> {
    let fresh = rows_of(map).map_err(Error::Parse)?;
    save_rows(model, BLOCKS.iter().zip(&fresh), mark, Some(languages))
}

/// Grava só o bloco `block` de `map` no banco em `model`, com a marca
/// `mark`, como [`save_at`]; os outros blocos ficam como estão, com as marcas
/// deles. É a passada do scan que só refaz o censo. Sem as línguas, o bloco
/// dos arquivos, das declarações ou das ligações gravado assim deixa o
/// índice de busca para a primeira busca refazer.
///
/// # Errors
///
/// A chave que não cabe na coluna dela ([`Error::Parse`]) e a falha do banco.
pub fn save_block_at(model: &Path, block: &MapBlock, map: &Value, mark: &str) -> Result<bool> {
    let fresh: BlockRows =
        block.tables.iter().map(|table| rows(table, map)).collect::<std::result::Result<_, _>>().map_err(Error::Parse)?;
    save_rows(model, [(block, &fresh)], mark, None)
}

/// Grava a história das declarações de um arquivo, `lineage`, no mapa em
/// `model`, que já tem de existir: troca só as linhas daquele arquivo, numa
/// transação; as dos outros arquivos ficam como estavam. Os títulos dos
/// commits de cada declaração entram no índice de busca na mesma transação,
/// só nos documentos daquele arquivo: o índice não se esvazia nem se refaz.
///
/// # Errors
///
/// O mapa que falta ou não se abre, e a falha do banco.
pub fn save_lineage_at(model: &Path, lineage: &FileLineage) -> Result<()> {
    save_lineages_at(model, std::slice::from_ref(lineage))
}

/// Grava a história de vários arquivos, `lineages`, como [`save_lineage_at`]
/// a de um, numa transação só: quem lê nunca vê um lote pela metade, e o
/// índice de busca é acertado uma vez para o lote todo.
///
/// # Errors
///
/// O mapa que falta ou não se abre, e a falha do banco.
pub fn save_lineages_at(model: &Path, lineages: &[FileLineage]) -> Result<()> {
    let mut changes = Vec::new();
    for lineage in lineages {
        changes.extend(lineage_changes(lineage)?);
    }
    let paths: Vec<&str> = lineages.iter().map(|lineage| lineage.path.as_str()).collect();
    let mut db = open_existing(model).map_err(|refusal| Error::Parse(format!("{refusal:?}")))?;
    db.write(|tx| {
        replace_in(tx, &changes)?;
        map_search::refresh_files(tx, &paths)?;
        map_revision::bump(tx)?;
        Ok(())
    })
}

/// As linhas que a história de um arquivo troca em cada tabela da sua
/// família, pelo caminho dele.
fn lineage_changes(lineage: &FileLineage) -> Result<Vec<Replacement<'static>>> {
    let path = lineage.path.as_str();
    let rows_in_map = serde_json::json!({
        "files": [{
            "path": path, "base": lineage.base, "last_commit": lineage.last_commit, "tip": lineage.tip,
            "mark": lineage.mark, "moves": lineage.moves, "comments": lineage.comments,
        }],
        "commits": lineage.commits.iter().map(|commit| serde_json::json!({
            "path": path, "id": commit.id, "at": commit.at, "title": commit.title, "pr": commit.pr,
            "files": (!commit.files.is_empty()).then_some(&commit.files),
        })).collect::<Vec<_>>(),
        "declarations": lineage.declarations.iter().map(|decl| serde_json::json!({
            "path": path, "name": decl.name, "nth": decl.nth, "commits": decl.commits,
            "comments": (!decl.comments.is_empty()).then_some(&decl.comments),
        })).collect::<Vec<_>>(),
    });
    let fresh: BlockRows =
        LINEAGE.tables.iter().map(|table| rows(table, &rows_in_map)).collect::<std::result::Result<_, _>>().map_err(Error::Parse)?;
    Ok(LINEAGE
        .tables
        .iter()
        .zip(fresh)
        .map(|(table, rows)| (table.name, "path", Sql::Text(path.to_string()), rows))
        .collect())
}

/// As linhas de cada tabela de cada bloco, na ordem de [`BLOCKS`].
type BlockRows = Vec<Vec<Row>>;

/// Grava as linhas `fresh` de cada bloco que elas trazem, com a marca
/// `mark`; o bloco que ficou igual, com a mesma marca, não se regrava.
/// Regravado um bloco de que o índice de busca lê — os arquivos, as
/// declarações ou as ligações, onde moram as chamadas —, ele se refaz nas
/// línguas `languages`; sem elas, fica sem línguas, e a primeira busca o
/// refaz nas dela.
fn save_rows<'b>(
    model: &Path,
    fresh: impl IntoIterator<Item = (&'b MapBlock, &'b BlockRows)>,
    mark: &str,
    languages: Option<&Languages>,
) -> Result<bool> {
    let head = head_of(model);
    if !head.is_empty() && head != SQLITE_HEADER {
        remove(model)?;
    }
    let mut db = open(model)?;
    let mut changed: Vec<(&MapBlock, &BlockRows)> = Vec::new();
    for (block, tables) in fresh {
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
        if changed.iter().any(|(block, _)| INDEXED_FROM.iter().any(|indexed| indexed.name() == block.name())) {
            match languages {
                Some(languages) => map_search::rebuild(tx, languages)?,
                None => map_search::forget(tx)?,
            }
        }
        // Refeitas as declarações, a marca do glossário cuja declaração mudou
        // de nome ou sumiu sai junto.
        if changed.iter().any(|(block, _)| block.name() == DECLS.name()) {
            map_glossary::drop_stale(tx)?;
        }
        map_revision::bump(tx)?;
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
/// vai para o banco, sem marca em bloco nenhum e sem o índice de busca, que a
/// primeira busca faz nas línguas dela; o texto que não se entende
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
        Some(fresh) => save_rows(model, BLOCKS.iter().zip(&fresh), "", None).map(|_| ()),
        None => crate::io::fs::write_atomic(model, text.as_bytes()),
    }
}

/// Apaga o mapa em `model` e os arquivos ao lado dele, quando existem: o
/// diário ou o registro de gravações que sobrasse seria aplicado ao mapa novo.
fn remove(model: &Path) -> Result<()> {
    let beside = |suffix: &str| {
        let mut name = model.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    for path in [model.to_path_buf(), beside("-journal"), beside("-wal"), beside("-shm")] {
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

    /// As línguas de um projeto com o texto em português e o código em inglês.
    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

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

    /// A base do mapa é a que o projeto declara; sem declaração, a que o
    /// servidor aponta como padrão; sem servidor, a do checkout; o checkout
    /// solto de branch, sem servidor, não tem base. O git só é lido: nem a
    /// configuração dele nem a do projeto ganham uma linha.
    #[test]
    fn the_base_of_the_map_is_the_declared_one_then_the_default_of_the_server_then_the_checkout() {
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
        git(&["init", "-q", "-b", "trunk"]);
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        let first = git(&["rev-parse", "HEAD"]);
        std::fs::write(repo.join("a.txt"), "b\n").unwrap();
        git(&["commit", "-q", "-am", "second"]);
        let second = git(&["rev-parse", "HEAD"]);
        git(&["branch", "develop", &first]);
        let config_before = std::fs::read_to_string(repo.join(".git/config")).unwrap();
        let named = |base: Base| (base.name, base.tip);

        assert_eq!(named(base_of(repo)), ("trunk".into(), second.clone()), "no declaration, no server: the branch of the checkout");

        git(&["update-ref", "refs/remotes/origin/main", &first]);
        git(&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
        assert_eq!(
            named(base_of(repo)),
            ("main".into(), first.clone()),
            "no declaration: the default branch of the server, at the tip the server has"
        );

        std::fs::write(repo.join("mustard.json"), r#"{"git": {"flow": {"*": "develop"}}}"#).unwrap();
        assert_eq!(named(base_of(repo)), ("develop".into(), first.clone()), "the declared base wins over the server default");
        std::fs::remove_file(repo.join("mustard.json")).unwrap();

        git(&["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
        git(&["checkout", "-q", "--detach"]);
        assert_eq!(named(base_of(repo)), (String::new(), String::new()), "a loose checkout with no server has no base");

        assert_eq!(std::fs::read_to_string(repo.join(".git/config")).unwrap(), config_before, "the git configuration was only read");
        assert!(!repo.join("mustard.json").exists(), "the project configuration was only read");
    }

    /// O nome do arquivo e o caminho saem do mesmo texto, e o diário, o
    /// registro de gravações e a memória compartilhada são o nome do mapa com
    /// o fim que o SQLite dá.
    #[test]
    fn the_file_name_and_the_path_come_from_one_text() {
        assert_eq!(format!("{MAP_DIR}/{MAP_FILE_NAME}"), MAP_FILE);
        assert!(model_path(Path::new("raiz")).ends_with(MAP_FILE));
        assert_eq!(MAP_JOURNAL_FILE_NAME, format!("{MAP_FILE_NAME}-journal"));
        assert_eq!(MAP_WAL_FILE_NAME, format!("{MAP_FILE_NAME}-wal"));
        assert_eq!(MAP_SHARED_FILE_NAME, format!("{MAP_FILE_NAME}-shm"));
    }

    /// A listagem dá, pelo caminho relativo à pasta lida, o blob do que cada
    /// arquivo guarda agora: o do índice para o intocado, o do conteúdo para
    /// o editado e para o novo, nada para o apagado; o próprio mapa e os
    /// arquivos que o SQLite põe ao lado dele ficam de fora, e a marca muda
    /// com o conteúdo.
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
        for beside in [MAP_JOURNAL_FILE_NAME, MAP_WAL_FILE_NAME, MAP_SHARED_FILE_NAME] {
            std::fs::write(root.join(MAP_DIR).join(beside), "ao lado").unwrap();
        }
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
    /// guarda — entre elas as que o scan antigo gravava e o banco deixou de
    /// guardar: as camadas, os pontos de registro e as dependências de cada
    /// projeto.
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
                     "doc": "Soma um.", "whole_doc": "Soma um. E devolve o total.", "body_comment": "o total vem de beta",
                     "body_names": "alpha s beta", "signature": "fn alpha(s: &str) -> \"a\\b\"\n\t\u{1} ação",
                     "used_by": ["src/b.rs:2:beta"]},
                    {"kind": "struct", "name": "Alpha", "line": 5, "end_line": 6, "supertypes": ["Base"],
                     "members": ["src/a.rs:7:run"]},
                    {"kind": "method", "name": "run", "line": 7, "end_line": 7, "owner": ["Alpha"], "contract": ["Base"],
                     "implements": ["src/b.rs:3:run"], "implemented_by": ["src/c.rs:9:run"]}
                 ],
                 "deps": ["src/b.rs"], "calls": ["beta:2", "b.beta:4"], "call_paths": {"crate::b": ["beta:4"]},
                 "other_call_paths": {"std::fs": ["fs.read:5"]}, "brought": {"std::fs::{self}": {"fs": "fs"}},
                 "reexports": {"io::leitor::Leitor": {"Leitor": "Leitor"}}, "unbound_heads": ["std"],
                 "routes": [{"method": "GET", "path": "pedidos/{}", "written": "/pedidos/:id", "handler": "alpha", "line": 1,
                             "framework": "axum",
                             "called_by": ["web/tela.ts:3:carregar",
                                           {"at": "web/outra.ts:5:salvar", "candidates": ["src/a.rs:1:alpha"]}]}],
                 "route_links": {"mounts": [{"framework": "axum", "target": "rotas", "line": 4, "written": "api", "path": "api"}],
                                 "clients": [{"framework": "axios", "name": "api", "written": "/api", "path": "api"}]},
                 "route_calls": [{"method": "GET", "path": "pedidos/{}", "written": "/pedidos/${id}", "line": 3,
                                  "owner": "carregar", "framework": "axios", "via": "api"}],
                 "file_doc": "O leitor dos pedidos.", "file_comment": "o fim do leitor", "file_doc_in_body": 3},
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
            "state": {"head": "abc", "base": "dev", "base_tip": "fed", "listing": "00ff-2", "inputs": {"Cargo.toml": "b1"}},
            "history": {"base": "dev", "paths": ["src/a.rs", "src/b.rs"],
                        "commits": [{"id": "c1", "at": 10, "title": "Cria o leitor (#12)", "pr": 12, "added": [0, 1]}, {"id": "c2", "at": 20, "changed": [1]}]}
        })
    }

    /// O blob que o mapa guardou vem pelo caminho pedido, e só dos arquivos
    /// pedidos que o mapa tem.
    #[test]
    fn the_blob_the_map_read_comes_back_for_the_files_asked() {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        assert!(save_at(&model, &scan_map(), "scan 1", &languages()).unwrap());
        let blobs = blobs_of(&model, &["src/a.rs", "src/nao_existe.rs"]).unwrap();
        assert_eq!(blobs.len(), 1, "{blobs:?}");
        assert_eq!(blobs["src/a.rs"], "a1");
        assert!(blobs_of(&model, &[]).unwrap().is_empty());
        assert!(blobs_of(&dir.path().join("nada.db"), &["src/a.rs"]).is_err(), "sem mapa, recusa");
    }

    /// O mapa do scan volta do banco igual, fora as chaves que nenhuma coluna
    /// guarda.
    #[test]
    fn the_scan_map_comes_back_from_the_database_as_it_went_in() {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        let map = scan_map();
        assert!(save_at(&model, &map, "scan 1", &languages()).unwrap());
        let stored = read_stored_at(&model).unwrap();
        let back: Value = serde_json::from_str(&stored.json).unwrap();

        let mut expected = map;
        let Value::Object(top) = &mut expected else { unreachable!() };
        top.remove("shared_contracts");
        top["graph"]
            .as_object_mut()
            .unwrap()
            .retain(|key, _| !["cyclic", "top_fan_out", "layers", "touchpoints"].contains(&key.as_str()));
        top["coverage"].as_object_mut().unwrap().retain(|key, _| key == "skipped_build_dirs");
        top["projects"][0].as_object_mut().unwrap().remove("dependencies");
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
        assert_eq!(read.history.commits[0].title, "Cria o leitor (#12)");
        assert_eq!((read.history.base.as_str(), read.history.commits[0].pr, read.history.commits[1].pr), ("dev", Some(12), None));
        // A leitura do resumo traz o mesmo histórico, títulos inclusive.
        assert_eq!(read_for(dir.path(), Need::Summary).unwrap().history, read.history);
        // A leitura das partes traz o arquivo com os trechos de teste e as
        // declarações dele, com o tipo, o nome e as linhas.
        let asked = read_for(dir.path(), Need::Parts("src/a.rs")).unwrap();
        assert_eq!(asked.modules.len(), 1);
        assert_eq!(asked.modules[0].test_lines, read.modules[0].test_lines);
        let lines = |module: &MapModule| -> Vec<(String, String, u64, u64)> {
            module.declarations.iter().map(|d| (d.kind.clone(), d.name.clone(), d.line, d.end_line)).collect()
        };
        assert_eq!(lines(&asked.modules[0]), lines(&read.modules[0]));
        assert!(read_for(dir.path(), Need::Parts("src/zz.rs")).unwrap().modules.is_empty());
    }

    /// Com o mesmo mapa e a mesma marca, nada se grava: o arquivo fica com os
    /// mesmos bytes. Um arquivo que muda regrava o mapa; a marca nova, também.
    #[test]
    fn saving_the_same_map_again_writes_nothing() {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        let map = scan_map();
        assert!(save_at(&model, &map, "scan 1", &languages()).unwrap());
        let before = std::fs::read(&model).unwrap();
        assert!(!save_at(&model, &map, "scan 1", &languages()).unwrap(), "nothing changed, nothing is written");
        assert_eq!(std::fs::read(&model).unwrap(), before);

        let mut changed = map.clone();
        changed["modules"][1]["loc"] = json!(21);
        assert!(save_at(&model, &changed, "scan 1", &languages()).unwrap());
        let back: Value = serde_json::from_str(&read_stored_at(&model).unwrap().json).unwrap();
        assert_eq!(back["modules"][1]["loc"], json!(21));
        assert!(save_at(&model, &changed, "scan 2", &languages()).unwrap(), "a new mark is written");
        assert!(read_stored_at(&model).unwrap().marks.values().all(|mark| mark == "scan 2"));
    }

    /// O estado da leitura traz o censo inteiro e, de cada arquivo, só o
    /// caminho, o blob e os sinais, com a marca de cada bloco, mesmo com uma
    /// coluna estragada que ele não lê. A gravação de um bloco só troca as
    /// linhas e a marca dele; os outros ficam como estavam.
    #[test]
    fn the_reading_state_is_read_alone_and_one_block_is_written_alone() {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        let mut map = scan_map();
        map["modules"][0]["signals"] = json!(["extends Controller"]);
        save_at(&model, &map, "scan 1", &languages()).unwrap();
        open(&model).unwrap().conn().execute("UPDATE \"links\" SET \"deps\" = '{broken'", []).unwrap();
        assert!(read_at(&model).is_err(), "o mapa inteiro recusa a coluna estragada");

        let state = read_state_at(&model).unwrap();
        let back: Value = serde_json::from_str(&state.json).unwrap();
        let mut expected = map.clone();
        expected["projects"][0].as_object_mut().unwrap().remove("dependencies");
        for key in ["root", "state", "manifests", "projects", "languages", "frameworks", "skeleton", "detected_stacks"] {
            assert_eq!(back[key], expected[key], "{key}");
        }
        assert_eq!(back["coverage"], json!({"skipped_build_dirs": ["target"]}));
        assert_eq!(
            back["modules"],
            json!([
                {"path": "src/a.rs", "blob": "a1", "signals": ["extends Controller"], "declarations": []},
                {"path": "src/b.rs", "declarations": []}
            ])
        );
        assert!(back.get("graph").is_none() && back.get("history").is_none(), "{back}");
        assert_eq!(state.marks.len(), BLOCKS.len());

        let mut census = back;
        census["state"]["listing"] = json!("11aa-3");
        census["detected_stacks"] = json!([{"name": "laravel", "confidence": 0.5, "signals": ["path:artisan"]}]);
        assert!(save_block_at(&model, &CENSUS, &census, "scan 2").unwrap());
        assert!(!save_block_at(&model, &CENSUS, &census, "scan 2").unwrap(), "o mesmo censo não se regrava");
        let again = read_state_at(&model).unwrap();
        let written: Value = serde_json::from_str(&again.json).unwrap();
        assert_eq!(written["state"]["listing"], json!("11aa-3"));
        assert_eq!(written["detected_stacks"][0]["name"], json!("laravel"));
        assert_eq!(written["modules"], census["modules"], "os arquivos ficam como estavam");
        for block in &BLOCKS {
            let expected = if block.name() == CENSUS.name() { "scan 2" } else { "scan 1" };
            assert_eq!(again.marks[block.name()], expected, "{}", block.name());
        }
        let deps: String =
            open(&model).unwrap().conn().query_row("SELECT \"deps\" FROM \"links\" LIMIT 1", [], |row| row.get(0)).unwrap();
        assert_eq!(deps, "{broken", "as ligações não se regravam");
    }

    /// O arquivo que não é um banco é trocado pelo mapa na gravação do scan.
    #[test]
    fn a_file_that_is_not_a_database_is_replaced_by_the_map() {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        write_text(dir.path(), "{not json").unwrap();
        assert!(save_at(&model, &scan_map(), "scan 1", &languages()).unwrap());
        assert_eq!(read(dir.path()).unwrap().modules.len(), 2);
    }

    /// O despejo traz uma entrada por tabela, na ordem dos blocos, e as
    /// linhas de cada uma.
    #[test]
    fn the_dump_has_one_entry_per_table() {
        let dir = tempdir().unwrap();
        save_at(&model_path(dir.path()), &scan_map(), "scan 1", &languages()).unwrap();
        let dump = dump(dir.path()).unwrap();
        let names: Vec<&str> = dump.as_array().unwrap().iter().map(|entry| entry["table"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "census", "projects", "languages", "manifests", "skeleton", "files", "decls", "texts", "routes", "links",
                "graph", "fan_in", "history_base", "history_paths", "commits", "lineage_files", "lineage_commits",
                "lineage_decls", "pr_texts", "pr_comments", "pr_commits", "spec_items", "spec_commits", "spec_pulls",
                "spec_marks", "glossary_asks", "glossary_marks", "notes", "blocks"
            ]
        );
        let decls = &dump[6]["rows"];
        assert_eq!(decls[0]["file"], json!("src/a.rs"));
        assert_eq!(decls[0]["used_by"], json!(["src/b.rs:2:beta"]));
        assert_eq!(decls[1]["doc"], Value::Null);
    }

    /// O que o provedor disse dos pull requests fica no mapa quando o scan o
    /// grava de novo, e a gravação do mesmo número troca o que havia dele: a
    /// história de um arquivo lê os comentários presos a ele, o número dado
    /// ao commit sem número e o texto dos pull requests dos commits dela.
    #[test]
    fn the_pull_request_rows_survive_a_new_scan_and_are_replaced_by_number() {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        save_at(&model, &scan_map(), "scan 1", &languages()).unwrap();
        let comment = |line: u64, body: &str| PullComment {
            number: 7,
            commit: "c0ffee1234".to_string(),
            path: "src/a.rs".to_string(),
            line,
            body: body.to_string(),
        };
        let text = |title: &str| PullText { number: 7, title: title.to_string(), body: "Descrição.".to_string(), etag: "W/\"e1\"".to_string(), through: "aaaa".to_string() };
        save_pull_at(&model, &text("Primeiro"), &[comment(3, "velho"), comment(4, "velho também")]).unwrap();
        save_pull_at(&model, &text("Grava o pagamento"), &[comment(3, "cuidado com o arredondamento")]).unwrap();
        save_pull_commits_at(&model, &[PullOfCommit { id: "bbbb".to_string(), pr: 9 }, PullOfCommit { id: "cccc".to_string(), pr: 0 }]).unwrap();
        keep_pull_at(&model, 7, "dddd").unwrap();
        let mut again = scan_map();
        again["modules"][0]["loc"] = json!(99);
        save_at(&model, &again, "scan 2", &languages()).unwrap();

        let sources = pull_sources_at(&model).unwrap();
        let texts: Vec<(u32, &str, &str)> = sources.texts.iter().map(|t| (t.number, t.etag.as_str(), t.through.as_str())).collect();
        assert_eq!(texts, [(7, "W/\"e1\"", "dddd")], "{sources:?}");
        let asked: Vec<(&str, u32)> = sources.asked.iter().map(|c| (c.id.as_str(), c.pr)).collect();
        assert_eq!(asked, [("bbbb", 9), ("cccc", 0)]);
        let comments = pull_comments_at(&model, "src/a.rs").unwrap();
        assert_eq!(comments, [comment(3, "cuidado com o arredondamento")], "a gravação do mesmo número troca os comentários");
        assert!(pull_comments_at(&model, "src/b.rs").unwrap().is_empty());
        let pull = read_for_at(&model, Need::Pull(7)).unwrap();
        assert_eq!(pull.pulls.texts.len(), 1);
        assert_eq!((pull.pulls.texts[0].title.as_str(), pull.pulls.texts[0].body.as_str()), ("Grava o pagamento", "Descrição."));
        assert!(read_for_at(&model, Need::Pull(8)).unwrap().pulls.texts.is_empty());
    }

    /// As tabelas do banco da pasta e as colunas de cada uma.
    fn tables_and_columns(conn: &Connection) -> BTreeMap<String, Vec<String>> {
        let mut stmt = conn
            .prepare(
                "SELECT t.name, c.name FROM pragma_table_list AS t JOIN pragma_table_info(t.name) AS c \
                 WHERE t.schema = 'main' AND t.type = 'table' AND t.name NOT LIKE 'sqlite_%' ORDER BY t.name, c.cid",
            )
            .unwrap();
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).unwrap();
        for row in rows {
            let (table, column) = row.unwrap();
            out.entry(table).or_default().push(column);
        }
        out
    }

    /// O banco gravado no formato de antes — a coluna das dependências na
    /// tabela dos projetos, as tabelas das camadas e dos pontos de registro
    /// no grafo — perde na troca de versão o que ninguém mais lê, e o mapa
    /// volta inteiro na gravação seguinte.
    #[test]
    fn a_database_of_the_old_format_loses_what_nobody_reads_on_the_version_change() {
        let dir = tempdir().unwrap();
        let model = model_path(dir.path());
        assert!(save_at(&model, &scan_map(), "scan 1", &languages()).unwrap());
        {
            let conn = Connection::open(&model).unwrap();
            conn.execute_batch(
                "ALTER TABLE projects ADD COLUMN dependencies TEXT; \
                 CREATE TABLE layers(name TEXT, modules INTEGER); INSERT INTO layers VALUES ('L0', 2); \
                 CREATE TABLE touchpoints(module TEXT, fan_out INTEGER, breadth INTEGER); \
                 INSERT INTO touchpoints VALUES ('src/a.rs', 1, 1); \
                 UPDATE blocks SET version = version - 1 WHERE name IN ('census', 'graph');",
            )
            .unwrap();
            let before = tables_and_columns(&conn);
            assert!(before.contains_key("layers") && before.contains_key("touchpoints"), "{before:?}");
            assert!(before["projects"].contains(&"dependencies".to_string()), "{before:?}");
        }

        assert!(save_at(&model, &scan_map(), "scan 2", &languages()).unwrap());
        let after = tables_and_columns(&Connection::open(&model).unwrap());
        assert!(!after.contains_key("layers") && !after.contains_key("touchpoints"), "{after:?}");
        assert!(!after["projects"].contains(&"dependencies".to_string()), "{:?}", after["projects"]);
        assert!(after["manifests"].contains(&"dependencies".to_string()), "{:?}", after["manifests"]);
        let back: Value = serde_json::from_str(&read_stored_at(&model).unwrap().json).unwrap();
        assert_eq!(back["modules"].as_array().map(Vec::len), Some(2));
        assert_eq!(back["graph"], json!({"nodes": 2, "edges": 1, "top_fan_in": [{"module": "src/b.rs", "degree": 1}]}));
        assert_eq!(back["manifests"][0]["dependencies"], json!(["serde"]));
        assert_eq!(back["projects"][0].get("dependencies"), None, "{}", back["projects"]);
    }
}
