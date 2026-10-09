//! Private loopback worker, one checkout/language/binary/environment per worker.
//! Serial requests, OS-random token, bounded frames and idle shutdown. Only
//! compiler sessions are reused: answers and source receipts are never cached.
use super::{DEADLINE, Location, Relation, Resolution, Session, frame};
use crate::io::fs::lock::LockedFile;
use crate::platform::code_tools::code_tool_for_language;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
mod snapshot;
const IDLE:Duration=Duration::from_secs(90);

#[derive(Deserialize,Serialize)]
struct Endpoint {address:std::net::SocketAddr,token:String}
#[derive(Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
struct Query {token:String,file:String,line:u64,column:u64,relation:String,limit:usize}

fn key(tree:&Path,language:&str,exe:&Path)->String {
    let mut hash=crate::io::sha256::Sha256::new();
    hash.update(format!("{}\0{language}\0{}\0{:?}\0{:?}",tree.display(),exe.display(),std::fs::metadata(exe).and_then(|m|m.modified()).ok(),
        std::env::vars_os().filter(|(key,_)|!key.to_string_lossy().starts_with("MUSTARD_")).collect::<std::collections::BTreeMap<_,_>>()).as_bytes());
    hash.hex_digest()
}
fn directory(tree:&Path,language:&str,exe:&Path)->Result<PathBuf,String> {
    let path=crate::ClaudePaths::for_project(tree).map_err(|e|e.to_string())?.claude_dir().join(".cache/lsp").join(key(tree,language,exe));
    std::fs::create_dir_all(&path).map_err(|e|e.to_string())?;
    if !path.canonicalize().map_err(|e|e.to_string())?.starts_with(tree) {return Err("lsp-worker-outside-checkout".into());}
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path,std::fs::Permissions::from_mode(0o700)).map_err(|e|e.to_string())?;
    }
    Ok(path)
}
fn send(stream:&mut TcpStream,message:&Value)->Result<(),String> {
    let text=message.to_string();write!(stream,"Content-Length: {}\r\n\r\n{text}",text.len()).map_err(|e|e.to_string())
}
fn call(path:&Path,location:&Location<'_>,relation:Relation,limit:usize)->Result<Resolution,String> {
    let bytes=std::fs::read(path.join("endpoint.json")).map_err(|e|e.to_string())?;
    if bytes.len()>8192{return Err("lsp-worker-invalid-endpoint".into());}
    let endpoint:Endpoint=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
    if endpoint.address.ip()!=std::net::Ipv4Addr::LOCALHOST || endpoint.token.len()!=64 {return Err("lsp-worker-invalid-endpoint".into());}
    let mut stream=TcpStream::connect_timeout(&endpoint.address,Duration::from_millis(150)).map_err(|e|e.to_string())?;
    stream.set_read_timeout(Some(DEADLINE+Duration::from_secs(15))).map_err(|e|e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(2))).map_err(|e|e.to_string())?;
    let query=Query{token:endpoint.token,file:location.file.into(),line:location.line,column:location.column_bytes,
        relation:match relation {Relation::Definitions=>"definitions",Relation::References=>"references",Relation::Implementations=>"implementations"}.into(),limit};
    send(&mut stream,&serde_json::to_value(query).map_err(|e|e.to_string())?)?;
    let reply=frame(&mut BufReader::new(stream))?;
    if let Some(error)=reply["error"].as_str() {return Err(format!("lsp-worker-query: {error}"));}
    serde_json::from_value(reply["result"].clone()).map_err(|e|e.to_string())
}

pub(super) fn resolve(tree:&Path,location:&Location<'_>,relation:Relation,limit:usize,language:&str)->Option<Result<Resolution,String>> {
    let exe=std::env::current_exe().ok()?;
    if !exe.file_stem()?.to_str()?.starts_with("mustard-rt") {return None;}
    let tree=tree.canonicalize().ok()?;
    let path=directory(&tree,language,&exe).ok()?;
    match call(&path,location,relation,limit) {
        Ok(result)=>return Some(Ok(result)),
        Err(error) if error.starts_with("lsp-worker-query:")=>return Some(Err(error)),
        Err(_)=>{}
    }
    let launch=LockedFile::exclusive_if_free(&path.join("launch.lock")).ok()??;
    let mut command=crate::platform::process::command(exe.to_str()?);
    #[cfg(unix)] {use std::os::unix::process::CommandExt;command.process_group(0);}
    let child=command.arg("--mustard-lsp-worker").arg(&tree).arg(language)
        .env("MUSTARD_RT_DELEGATED","1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()?;
    // Reap worker after idle shutdown without holding the command invocation.
    std::thread::spawn(move || {let mut child=child;let _=child.wait();});
    let start=Instant::now();
    while start.elapsed()<Duration::from_secs(2) {
        match call(&path,location,relation,limit) {
            Ok(result)=>return Some(Ok(result)),
            Err(error) if error.starts_with("lsp-worker-query:")=>return Some(Err(error)),
            Err(_)=>std::thread::sleep(Duration::from_millis(20)),
        }
    }
    drop(launch);
    None
}

/// Internal runtime face; deliberately not a model-facing command.
pub fn run(tree:&Path,language:&str)->Result<(),String> {
    let tree=tree.canonicalize().map_err(|e|e.to_string())?;
    let tool=code_tool_for_language(language).ok_or("lsp-language-unsupported")?;
    let exe=std::env::current_exe().map_err(|e|e.to_string())?;
    let path=directory(&tree,language,&exe)?;
    let Some(_lifetime)=LockedFile::exclusive_if_free(&path.join("worker.lock")).map_err(|e|e.to_string())? else {return Ok(());};
    let listener=TcpListener::bind("127.0.0.1:0").map_err(|e|e.to_string())?;
    listener.set_nonblocking(true).map_err(|e|e.to_string())?;
    let mut random=[0_u8;32];getrandom::fill(&mut random).map_err(|e|e.to_string())?;
    let token=random.iter().fold(String::new(),|mut out,b|{use std::fmt::Write as _;let _=write!(out,"{b:02x}");out});
    let endpoint=Endpoint{address:listener.local_addr().map_err(|e|e.to_string())?,token};
    crate::io::fs::write_atomic(path.join("endpoint.json"),&serde_json::to_vec(&endpoint).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let mut last=Instant::now();
    let mut session:Option<Session>=None;
    let mut context=String::new();
    let mut snapshot=snapshot::Snapshot::default();
    while last.elapsed()<IDLE {
        let mut stream=match listener.accept() {
            Ok((stream,_))=>stream,
            Err(error) if error.kind()==std::io::ErrorKind::WouldBlock=>{std::thread::sleep(Duration::from_millis(25));continue;},
            Err(error)=>return Err(error.to_string()),
        };
        stream.set_read_timeout(Some(Duration::from_secs(2))).map_err(|e|e.to_string())?;
        stream.set_write_timeout(Some(Duration::from_secs(2))).map_err(|e|e.to_string())?;
        let query=frame(&mut BufReader::new(stream.try_clone().map_err(|e|e.to_string())?))
            .and_then(|v|serde_json::from_value::<Query>(v).map_err(|e|e.to_string()));
        let Ok(query)=query else {continue;};
        if query.token!=endpoint.token || query.file.len()>4096 {continue;}
        last=Instant::now();
        let result=(|| {
            snapshot.observe(&query.file);
            let Ok(before)=snapshot.fingerprint(&tree) else {
                session=None;return super::resolve_with(&tree,&Location{file:&query.file,line:query.line,column_bytes:query.column},Relation::parse(&query.relation)?,query.limit,tool,language);
            };
            let warm=session.is_some() && before==context;
            if !warm {session=None;session=Some(Session::start(&tree,tool)?);context.clone_from(&before);}
            let location=Location{file:&query.file,line:query.line,column_bytes:query.column};
            let mut result=session.as_mut().ok_or("lsp-session-unavailable")?.resolve(&tree,&location,Relation::parse(&query.relation)?,query.limit,tool,language)?;
            if snapshot.fingerprint(&tree)?!=before {return Err("lsp-context-changed-during-query".into());}
            result.status=format!("live-language-server; {} session; source/config hashes and dependency metadata; no answer cache",if warm{"reused"}else{"new"});
            Ok::<_,String>(result)
        })();
        let reply=match result {Ok(result)=>json!({"result":result}),Err(error)=>{session=None;json!({"error":error})}};
        let _=send(&mut stream,&reply);
    }
    drop(session);
    let _=std::fs::remove_file(path.join("endpoint.json"));
    Ok(())
}
