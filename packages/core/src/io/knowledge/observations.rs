//! Durable, verified source facts. Choices and inferred meaning are never facts.
use crate::domain::knowledge::resources::Registry;
use crate::io::map_db::{BUSY_WAIT, Block, Kept, Kind, Later, MapDb, table_exists};
use crate::io::project_map as store;
use crate::io::sha256::Sha256;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

/// What the search learned, per tree: the main checkout and each wave copy
/// write into the main map, keyed by the hash of the tree's folder, and
/// `search_trees` keeps that folder so a copy that is gone can be forgotten.
pub(crate) const BLOCK:Block=Block {
    name:"search_facts",version:2,tables:&["search_files","search_facts","search_trees"],
    schema:"CREATE TABLE IF NOT EXISTS search_files(tree TEXT NOT NULL,path TEXT NOT NULL,sha256 TEXT NOT NULL,indexed TEXT NOT NULL DEFAULT '',PRIMARY KEY(tree,path));
        CREATE TABLE IF NOT EXISTS search_facts(tree TEXT NOT NULL,path TEXT NOT NULL,line INTEGER NOT NULL,sha256 TEXT NOT NULL,text TEXT NOT NULL,PRIMARY KEY(tree,path,line));
        CREATE INDEX IF NOT EXISTS search_facts_source ON search_facts(tree,path,sha256);
        CREATE TABLE IF NOT EXISTS search_trees(tree TEXT PRIMARY KEY,path TEXT NOT NULL);",
    kind:Kind::Written(convert),
};
fn convert(conn: &Connection, _version: u32) -> crate::platform::error::Result<()> {
    conn.execute_batch(BLOCK.schema)?;
    Ok(())
}
fn hash(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    digest.hex_digest()
}
/// Quanto a gravação do que a busca aprendeu espera a trava num mapa com bloco
/// a pôr em dia. Quem a segura é, quase sempre, a passada do scan refazendo o
/// bloco, por dezenas de segundos num projeto grande: a busca não espera por
/// ela, e a fonte que não se gravou segue pendente para a passada.
const OUTDATED_WAIT: Duration = Duration::from_millis(200);

/// Quanto a busca espera a trava do mapa para gravar, de uma vez, o que
/// aprendeu ([`Later::keep`]): a mesma espera curta do mapa com bloco a pôr
/// em dia. Quem segura a trava é outra busca, a busca de outra cópia no mesmo
/// mapa ou a passada do scan; esperar por ela seguraria a resposta, e o que
/// não se grava se aprende de novo na próxima busca.
pub const LEARNING_WAIT: Duration = OUTDATED_WAIT;

/// Quantas fontes de um resultado o aprendizado lê, e quantos bytes delas no
/// máximo: a fonte além disso não entra no que a busca aprendeu.
pub(crate) const MAX_SOURCES: usize = 96;
pub(crate) const MAX_SOURCE_BYTES: usize = 12 * 1024 * 1024;

fn tree_key(tree: &Path) -> String {
    hash(tree.to_string_lossy().as_bytes())
}

/// A chave, em [`Later`], da fonte que a passada terminada confirmou.
fn indexed_key(tree: &str, file: &str, digest: &str) -> String {
    format!("indexed\0{tree}\0{file}\0{digest}")
}

/// A chave, em [`Later`], da versão de fonte já juntada para gravar.
fn source_key(tree: &str, file: &str, digest: &str) -> String {
    format!("source\0{tree}\0{file}\0{digest}")
}

/// A chave, em [`Later`], da linha de fonte já juntada para gravar.
fn fact_key(tree: &str, file: &str, line: i64, digest: &str, text: &str) -> String {
    format!("fact\0{tree}\0{file}\0{line}\0{digest}\0{text}")
}

/// O que [`learn`] apurou: o relato e se o mapa tem bloco a pôr em dia.
struct Learned {
    learning: Value,
    outdated: bool,
}

/// Grava na hora o que `hits` ensinam, esperando a trava do mapa como toda
/// gravação. Learning is a durable write, unlike the best-effort edit hook.
/// Its old 50 ms timeout dropped discoveries while a normal scan/writer
/// committed. Use the database's bounded writer wait (5 s); persistent
/// contention is still reported by the caller, which preserves the native
/// search result. A busca não passa por aqui: junta o que aprende ([`learn`])
/// e grava no fim, sem esperar.
pub fn record(root: &Path, tree: &Path, hits: &[(String, u64, String)]) -> Result<Value, String> {
    let mut later = Later::default();
    let Learned { mut learning, outdated } = plan(root, tree, hits, &mut later)?;
    let kept = later.keep(if outdated { OUTDATED_WAIT } else { BUSY_WAIT });
    if !kept.left() {
        return Ok(learning);
    }
    let reason = kept.reason.unwrap_or_default();
    // O mapa que espera a passada segue pedindo por ela, com toda fonte
    // pendente, mesmo sem a gravação.
    if !outdated {
        return Err(reason);
    }
    learning["status"] = json!("not-stored");
    learning["reason"] = json!(reason);
    learning["new_facts"] = json!(0);
    Ok(learning)
}

/// O que `hits` ensinam, como [`record`] relata, com a gravação junta em
/// `later` em vez de feita: o mapa só se lê, e quem grava é quem guarda
/// `later`, no fim da busca.
pub fn learn(root: &Path, tree: &Path, hits: &[(String, u64, String)], later: &mut Later) -> Result<Value, String> {
    plan(root, tree, hits, later).map(|learned| learned.learning)
}

fn plan(root: &Path, tree: &Path, hits: &[(String, u64, String)], later: &mut Later) -> Result<Learned, String> {
    let path = store::model_path(root);
    if hits.is_empty() {
        let needs_scan=if path.is_file() {!structure_ready(&look(root)?)?}else{false};
        return Ok(Learned { learning: json!({"status":"no-source-facts","new_facts":0,"needs_scan":needs_scan,"source_hashes":{}}), outdated: false });
    }
    let registry = Registry::load()?;
    let mut sources = BTreeMap::new();
    let mut bytes = 0;
    for (file, _, _) in hits {
        if sources.contains_key(file) {
            continue;
        }
        if sources.len() >= MAX_SOURCES || bytes >= MAX_SOURCE_BYTES {
            break;
        }
        if let Some(text) = super::investigation::safe_read(tree, file, &registry) {
            bytes += text.len();
            sources.insert(file.clone(), (hash(text.as_bytes()), text));
        }
    }
    // Remember source versions from the whole result, then distribute stored
    // line witnesses across files instead of spending them on an early file.
    let mut by_file=BTreeMap::<&str,Vec<_>>::new();
    for hit in hits {if sources.contains_key(&hit.0) {by_file.entry(&hit.0).or_default().push(hit);}}
    let mut witnesses=Vec::new();let mut at=0;
    while witnesses.len()<256 {
        let mut added=false;
        for group in by_file.values() {
            if witnesses.len()>=256 {break;}
            if let Some(hit)=group.get(at) {witnesses.push(*hit);added=true;}
        }
        if !added {break;}at+=1;
    }
    // O mapa só se lê aqui: sem ele, nada está indexado nem aprendido.
    let db = if path.is_file() { Some(look(root)?) } else { None };
    // A block written by another program version is rebuilt by the next scan.
    // A source hash learned under the old schema cannot acknowledge it.
    let structure_ready = match &db { Some(db) => structure_ready(db)?, None => false };
    let outdated = match &db { Some(db) => store::outdated_block(db).map_err(|e| e.to_string())?, None => false };
    let known = match &db { Some(db) => table_exists(db.conn(), "search_files").map_err(|e| e.to_string())?, None => false };
    let key = tree_key(tree);
    let mut pending = Vec::new();
    // Só a versão de fonte que o mapa ainda não guarda se grava: a busca que
    // repete fontes conhecidas não pede a trava.
    let mut changed = Vec::new();
    for (file,(digest,_)) in &sources {
        let stored:Option<(String,String)>=match db.as_ref().filter(|_|known) {
            Some(db)=>db.conn().query_row("SELECT sha256,indexed FROM search_files WHERE tree=?1 AND path=?2",params![key,file],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(|e|e.to_string())?,
            None=>None,
        };
        if stored.as_ref().is_none_or(|(sha,_)|sha!=digest) && later.note(source_key(&key,file,digest)) {changed.push((file.clone(),digest.clone()));}
        // A scanned document or a code file with zero declarations is
        // still a current snapshot. Requiring a symbol causes a complete
        // scan on the first hit in every such file, despite identical bytes.
        // An outdated map may lack these tables; the scan answers first.
        let Some(db) = db.as_ref().filter(|_| structure_ready) else {pending.push(file.clone());continue;};
        let conn=db.conn();
        let indexed:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_symbols WHERE path=?1 AND sha256=?2)
            OR EXISTS(SELECT 1 FROM resource_index_files WHERE path=?1 AND sha256=?2)
            OR EXISTS(SELECT 1 FROM texts WHERE path=?1 AND CASE WHEN json_valid(analysis) THEN json_extract(analysis,'$.content_sha256') END=?2)",params![file,digest],|row|row.get(0)).map_err(|e|e.to_string())?;
        let learned=later.noted(&indexed_key(&key,file,digest)) || stored.is_some_and(|(_,indexed)|indexed==*digest);
        if !indexed && !learned {pending.push(file.clone());}
    }
    let (mut added, mut reused) = (0, 0);
    let mut facts = Vec::new();
    for (file,line,text) in &witnesses {
        let Some((digest,source))=sources.get(file) else {continue};
        if *line==0 || text.len()>4000 || source.lines().nth(line.saturating_sub(1) as usize)!=Some(text) {continue;}
        let Ok(line)=i64::try_from(*line) else{continue};
        let stored=match db.as_ref().filter(|_|known) {
            Some(db)=>db.conn().query_row("SELECT EXISTS(SELECT 1 FROM search_facts WHERE tree=?1 AND path=?2 AND line=?3 AND sha256=?4 AND text=?5)",params![key,file,line,digest,text],|row|row.get(0)).map_err(|e|e.to_string())?,
            None=>false,
        };
        if stored || !later.note(fact_key(&key,file,line,digest,text)) {reused+=1;continue;}
        added+=1;
        facts.push((file.clone(),line,digest.clone(),text.clone()));
    }
    let source_hashes=sources.iter().map(|(file,(digest,_))|(file,digest)).collect::<BTreeMap<_,_>>();
    let learning=json!({"status":"stored-current-source-facts","new_facts":added,"reused_facts":reused,"needs_scan":!pending.is_empty(),"pending_paths":pending,"omitted_line_witnesses":hits.len().saturating_sub(witnesses.len()),
        "tree":key,"source_hashes":source_hashes,"interpretations_recorded":0});
    if !changed.is_empty() || !facts.is_empty() {
        let place=tree.to_string_lossy().into_owned();
        later.defer(&path,root,BLOCK,move |tx| {
            tx.execute("INSERT INTO search_trees(tree,path) VALUES(?1,?2) ON CONFLICT(tree) DO UPDATE SET path=excluded.path",params![key,place])?;
            for (file,digest) in &changed {
                let old:Option<String>=tx.query_row("SELECT sha256 FROM search_files WHERE tree=?1 AND path=?2",params![key,file],|row|row.get(0)).optional()?;
                if old.as_ref().is_some_and(|old|old!=digest) {tx.execute("DELETE FROM search_facts WHERE tree=?1 AND path=?2",params![key,file])?;}
                tx.execute("INSERT INTO search_files(tree,path,sha256) VALUES(?1,?2,?3) ON CONFLICT(tree,path) DO UPDATE SET sha256=excluded.sha256",params![key,file,digest])?;
            }
            for (file,line,digest,text) in &facts {
                tx.execute("INSERT INTO search_facts(tree,path,line,sha256,text) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(tree,path,line) DO UPDATE SET sha256=excluded.sha256,text=excluded.text",params![key,file,line,digest,text])?;
            }
            Ok(())
        });
    }
    Ok(Learned { learning, outdated })
}

/// Esquece no mapa do projeto em `root` o que a busca aprendeu na cópia
/// `copy`, que saiu, e em toda árvore cuja pasta já não existe: a anotada
/// que sumiu e a que nenhuma pasta anotou, gravada antes de o mapa guardar
/// a pasta. O checkout principal nunca sai. Grava com a espera curta da
/// busca: com o mapa ocupado, nada sai agora, e a próxima remoção leva tudo.
pub fn forget_tree(root: &Path, copy: &Path) -> Kept {
    let path = store::model_path(root);
    if !path.is_file() {
        return Kept::default();
    }
    let main: Vec<String> = std::iter::once(root.to_path_buf()).chain(root.canonicalize().ok()).map(|tree| tree_key(&tree)).collect();
    let mut gone: BTreeSet<String> = std::iter::once(copy.to_path_buf()).chain(copy.canonicalize().ok()).map(|tree| tree_key(&tree)).collect();
    if let Ok(db) = look(root)
        && table_exists(db.conn(), "search_trees").unwrap_or(false)
        && let Ok(mut rows) = db.conn().prepare("SELECT tree,path FROM search_trees")
    {
        let trees = rows.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)));
        gone.extend(trees.into_iter().flatten().flatten().filter(|(_, place)| !Path::new(place).exists()).map(|(tree, _)| tree));
    }
    gone.retain(|tree| !main.contains(tree));
    let mut later = Later::default();
    later.defer(&path, root, BLOCK, move |tx| {
        let mut keep = main.clone();
        keep.extend(tx.prepare("SELECT tree FROM search_trees")?.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?);
        let unnamed: Vec<String> = tx.prepare("SELECT DISTINCT tree FROM search_files")?.query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?.into_iter().filter(|tree| !keep.contains(tree)).collect();
        for tree in gone.iter().chain(&unnamed) {
            for table in ["search_facts", "search_files", "search_trees"] {
                tx.execute(&format!("DELETE FROM {table} WHERE tree=?1"), params![tree])?;
            }
        }
        Ok(())
    });
    later.keep(LEARNING_WAIT)
}

/// The map as it is, only to read: nothing is created or brought up to date,
/// so reading what the search learned never waits for a writer.
fn look(root:&Path)->Result<MapDb,String> {
    MapDb::open_waiting(&store::model_path(root),root,&[],LEARNING_WAIT).map_err(|e|e.to_string())
}

fn structure_ready(db:&MapDb)->Result<bool,String> {
    if store::outdated_block(db).map_err(|e|e.to_string())? {return Ok(false);}
    for block in [&store::CENSUS,&store::FILES,&store::DECLS,&store::GRAPH,&store::RESOURCES] {
        if db.mark(block.name()).map_err(|e|e.to_string())?.is_none_or(|mark|mark.is_empty()) {return Ok(false);}
    }
    Ok(true)
}

/// Marca na hora só as versões de fonte que a passada nativa terminada foi
/// chamada a aprender ([`acknowledge`]), esperando a trava do mapa como toda
/// gravação.
pub fn scanned(root: &Path, tree: &Path, learning: &Value) -> Result<(), String> {
    let mut later = Later::default();
    acknowledge(root, tree, learning, &mut later)?;
    let kept = later.keep(BUSY_WAIT);
    match kept.reason {
        Some(reason) if kept.left() => Err(reason),
        _ => Ok(()),
    }
}

/// Junta em `later` a marca das versões de fonte que a passada nativa
/// terminada foi chamada a aprender. O mapa já tem de existir: a confirmação
/// nunca cria um. Só o bloco deste módulo é posto em dia na gravação: numa
/// cópia, `root` é o checkout principal, e o bloco do mapa dele gravado por
/// outra versão do programa fica para a passada do próprio checkout principal.
pub fn acknowledge(root: &Path, tree: &Path, learning: &Value, later: &mut Later) -> Result<(), String> {
    let path = store::model_path(root);
    store::existing(&path).map_err(|e| format!("{e:?}"))?;
    let key = tree_key(tree);
    let marks: Vec<(String, String)> = learning["source_hashes"].as_object().into_iter().flatten()
        .filter_map(|(file, digest)| Some((file.clone(), digest.as_str()?.to_string()))).collect();
    for (file, digest) in &marks {
        later.note(indexed_key(&key, file, digest));
    }
    later.defer(&path, root, BLOCK, move |tx| {
        for (file, digest) in &marks {
            tx.execute("UPDATE search_files SET indexed=?3 WHERE tree=?1 AND path=?2 AND sha256=?3", params![key, file, digest])?;
        }
        Ok(())
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier, mpsc};

    #[test]
    fn learning_waits_for_a_concurrent_writer_and_persists_the_original_discovery() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(root.join("a.rs"), "pub fn quartz() {}\n").unwrap();
        let mut db = MapDb::open(&store::model_path(&root), &root, &store::DB_BLOCKS).unwrap();
        let (send, receive) = mpsc::channel();
        let mut worker = None;
        db.write(|_| {
            let root = root.clone();
            worker = Some(std::thread::spawn(move || {
                send.send(record(&root, &root, &[("a.rs".into(), 1, "pub fn quartz() {}".into())])).unwrap();
            }));
            // The former 50 ms wait returned an error here; the new writer,
            // started while the transaction below holds the map, waits for
            // it to commit and then learns.
            Ok(())
        }).unwrap();
        let learning = receive.recv().unwrap().unwrap();
        worker.unwrap().join().unwrap();
        assert_eq!(learning["new_facts"], 1);
        let saved: (String, String) = db.conn().query_row("SELECT text,sha256 FROM search_facts WHERE path='a.rs' AND line=1", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(saved, ("pub fn quartz() {}".into(), hash(b"pub fn quartz() {}\n")));
    }

    #[test]
    fn concurrent_learners_commit_each_discovery_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        MapDb::open(&store::model_path(&root), &root, &store::DB_BLOCKS).unwrap();
        let barrier = Arc::new(Barrier::new(6));
        let mut workers = Vec::new();
        for i in 0..6 {
            let root = root.clone();
            let barrier = barrier.clone();
            let file = format!("module{i}.rs");
            std::fs::write(root.join(&file), "pub fn quartz() {}\n").unwrap();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                let hits = [(file, 1, "pub fn quartz() {}".into())];
                assert_eq!(record(&root, &root, &hits).unwrap()["new_facts"], 1);
                assert_eq!(record(&root, &root, &hits).unwrap()["reused_facts"], 1);
            }));
        }
        for worker in workers {worker.join().unwrap();}
        let db = MapDb::open(&store::model_path(&root), &root, &store::DB_BLOCKS).unwrap();
        let count: i64 = db.conn().query_row("SELECT count(*) FROM search_facts", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 6);
    }

    /// The rows each tree of the map holds, by tree key.
    fn learned_trees(root: &Path) -> BTreeMap<String, i64> {
        let db = MapDb::open(&store::model_path(root), root, &store::DB_BLOCKS).unwrap();
        let mut rows = db.conn().prepare("SELECT tree, count(*) FROM search_files GROUP BY tree").unwrap();
        rows.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn a_removed_copy_leaves_nothing_learned_in_the_main_map() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("main");
        let copy = dir.path().join("copies").join("slot-1");
        for tree in [&root, &copy] {
            std::fs::create_dir_all(tree).unwrap();
            std::fs::write(tree.join("a.rs"), "pub fn quartz() {}\n").unwrap();
        }
        MapDb::open(&store::model_path(&root), &root, &store::DB_BLOCKS).unwrap();
        for tree in [&root, &copy] {
            record(&root, tree, &[("a.rs".into(), 1, "pub fn quartz() {}".into())]).unwrap();
        }
        assert_eq!(learned_trees(&root).len(), 2);
        std::fs::remove_dir_all(&copy).unwrap();
        assert!(!forget_tree(&root, &copy).left());
        assert_eq!(learned_trees(&root).into_keys().collect::<Vec<_>>(), [tree_key(&root)], "only the main checkout keeps what it learned");
        let db = MapDb::open(&store::model_path(&root), &root, &store::DB_BLOCKS).unwrap();
        let facts: i64 = db.conn().query_row("SELECT count(*) FROM search_facts WHERE tree=?1", [tree_key(&copy)], |row| row.get(0)).unwrap();
        assert_eq!(facts, 0);
    }

    #[test]
    fn every_tree_whose_folder_is_gone_leaves_with_the_next_removal() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("main");
        let gone = dir.path().join("copies").join("old");
        let kept = dir.path().join("copies").join("alive");
        for tree in [&root, &gone, &kept] {
            std::fs::create_dir_all(tree).unwrap();
            std::fs::write(tree.join("a.rs"), "pub fn quartz() {}\n").unwrap();
        }
        MapDb::open(&store::model_path(&root), &root, &store::DB_BLOCKS).unwrap();
        for tree in [&root, &gone, &kept] {
            record(&root, tree, &[("a.rs".into(), 1, "pub fn quartz() {}".into())]).unwrap();
        }
        // The folder went away without anyone forgetting it, as a copy
        // deleted by hand or by an older program.
        std::fs::remove_dir_all(&gone).unwrap();
        assert!(!forget_tree(&root, &dir.path().join("copies").join("never-learned")).left());
        let trees = learned_trees(&root);
        assert!(!trees.contains_key(&tree_key(&gone)), "the vanished copy leaves: {trees:?}");
        assert!(trees.contains_key(&tree_key(&root)) && trees.contains_key(&tree_key(&kept)), "the living trees stay: {trees:?}");
    }

    #[test]
    fn forgetting_a_tree_waits_only_briefly_for_a_busy_map() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let mut db = MapDb::open(&store::model_path(&root), &root, &store::DB_BLOCKS).unwrap();
        db.write(|_| {
            // The forgetting comes back while this write still holds the map.
            let kept = forget_tree(&root, &root.join("copies").join("slot"));
            assert!(kept.left(), "the busy map leaves the forgetting for the next removal");
            Ok(())
        }).unwrap();
    }
}
