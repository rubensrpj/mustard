//! Cross an authorized host Read, without executing another tool or replacing
//! its result. Unknown formats and unavailable/stale owners leave Read intact.
use super::{Ctx, HookInput, Path, Value, first_evidence};
use mustard_core::platform::i18n::Locale;
use mustard_core::io::knowledge::investigation::Occurrence;
use std::fmt::Write;

pub(super) fn context(input:&HookInput,ctx:&Ctx)->Option<String> {
    if !ctx.config.search_answer() {return None;}
    let response=input.raw.get("tool_response")?;
    if response.get("type")?.as_str()?!="text"
        || ["truncated","isError","is_error","interrupted"].iter().any(|key|response.get(key).and_then(Value::as_bool)==Some(true)) {return None;}
    let file=response.get("file")?;
    if file.get("truncatedByTokenCap").and_then(Value::as_bool)==Some(true) {return None;}
    let content=file.get("content")?.as_str()?;
    let first=file.get("startLine")?.as_u64()?.checked_sub(1)?;
    let count=file.get("numLines")?.as_u64()?;
    let total=file.get("totalLines")?.as_u64()?;
    let last=first.checked_add(count)?;
    if count==0 || last>total || content.len()>128*1024 || content.lines().count() as u64!=count {return None;}
    let requested=input.tool_input.get("file_path")?.as_str()?;
    if first!=input.tool_input.get("offset").and_then(Value::as_u64).unwrap_or(1).saturating_sub(1)
        || input.tool_input.get("limit").and_then(Value::as_u64).is_some_and(|limit|count>limit) {return None;}
    let root=Path::new(&ctx.project_dir).canonicalize().ok()?;
    let base=Path::new(input.cwd.as_deref().unwrap_or(&ctx.project_dir)).canonicalize().ok()?;
    let requested=base.join(requested).canonicalize().ok()?;
    let resolved=crate::shared::paths::canonical(requested.to_str()?);
    let path=crate::shared::code_route::project_path(root.to_str()?,base.to_str()?,&resolved)?;
    let tree=path.tree.canonicalize().ok()?;
    let actual=base.join(file.get("filePath")?.as_str()?).canonicalize().ok()?;
    if actual!=path.abs.canonicalize().ok()? || !actual.starts_with(&tree) {return None;}
    let index=if mustard_core::io::project_map::model_path(&tree).is_file() {&tree}else{&root};
    if !mustard_core::io::project_map::model_path(index).is_file() {return None;}
    let facts:Vec<_>=content.lines().enumerate().map(|(at,text)|(path.rel.clone(),first+at as u64+1,text.to_owned())).collect();
    // Persist only source witnesses verified by the existing learning policy.
    // A changed/unmapped file is pending for the next native scan, not evidence
    // that the previous declaration is still current. No model or scan here.
    // Como a busca: o que se aprendeu grava no fim com a espera curta, e o
    // mapa com bloco a pôr em dia fica para a passada, sem segurar o agente.
    let mut later=mustard_core::io::map_db::Later::default();
    let _=mustard_core::io::knowledge::observations::learn(index,&tree,&facts,&mut later);
    let _=later.keep(mustard_core::io::knowledge::observations::LEARNING_WAIT);
    if mustard_core::io::project_map::needs_block_update(&mustard_core::io::project_map::model_path(index)) {return None;}
    let hits:Vec<_>=facts.iter().map(|(file,line,text)|Occurrence {file,line:*line,text}).collect();
    let report=mustard_core::io::knowledge::investigation::cross_hits(index,&tree,&hits).ok()?;
    let owners=report.get("symbols")?.as_array()?;
    if owners.is_empty() {return None;}
    let pt=ctx.config.language().text_or_default()==Locale::PtBr;
    let mut context=if pt {
        format!("Mustard: proprietários estáticos do trecho já lido ({}:{}-{last}).\n",path.rel,first+1)
    }else{format!("Mustard: static owners of the excerpt already read ({}:{}-{last}).\n",path.rel,first+1)};
    // O dono inteiro dentro da faixa, a leitura já mostrou: a nota leva só
    // o que começa ou termina fora dela e, sem nenhum, não sai.
    let mut partial=0;
    for owner in owners {
        let source=owner.get("source")?;
        let start=source.get("line")?.as_u64()?;
        let end=source.get("end_line")?.as_u64()?;
        if first<start && last>=end {continue;}
        partial+=1;
        let coverage=if pt {"intervalo parcial"}else{"partial range"};
        let _=writeln!(context,"- {}: {start}-{end} ({coverage}).",owner.get("name")?.as_str()?);
    }
    if partial==0 {return None;}
    let omitted=report.get("omitted_symbols")?.as_u64()?;
    if omitted>0 {let _=writeln!(context,"+{omitted} {}",if pt {"outros símbolos no trecho."}else{"other symbols in the excerpt."});}
    context.push_str(if pt {"Somente as linhas lidas foram conferidas. Relação estática não prova comportamento em execução."}else{"Only the delivered lines were checked. Static ownership does not prove runtime behavior."});
    if context.len()>1800 {return None;}
    first_evidence(&root,input,&report.to_string()).then_some(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SOURCE:&str="pub fn quartz() {\n    restore();\n}\n";
    fn read(root:&Path)->(HookInput,Ctx) {
        let input=serde_json::from_value(json!({"hook_event_name":"PostToolUse","tool_name":"Read","cwd":root,"session_id":"read-test",
            "tool_input":{"file_path":root.join("src/main.rs"),"offset":2,"limit":1},
            "tool_response":{"type":"text","file":{"filePath":root.join("src/main.rs"),"content":"    restore();\n","numLines":1,"startLine":2,"totalLines":3}}})).unwrap();
        (input,Ctx::for_test(root.to_str().unwrap(),None))
    }
    fn indexed()->tempfile::TempDir {
        let root=tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/main.rs"),SOURCE).unwrap();
        let mut digest=mustard_core::io::sha256::Sha256::new();digest.update(SOURCE.as_bytes());
        let mut raw=json!({"modules":[{"path":"src/main.rs","analysis":{"content_sha256":digest.hex_digest(),"parse_complete":true},
            "declarations":[{"name":"quartz","kind":"function","line":1,"end_line":3,"signature":"pub fn quartz()"}]}]});
        mustard_core::domain::knowledge::enrich(&mut raw);
        mustard_core::io::project_map::write_text(root.path(),&raw.to_string()).unwrap();
        root
    }
    #[test]
    fn executed_read_gains_compact_current_owners_once_without_replacing_its_result() {
        let root=indexed();let (mut input,ctx)=read(root.path());let original=input.raw["tool_response"].clone();
        let note=context(&input,&ctx).unwrap();
        assert!(note.contains("src/main.rs:2-2") && note.contains("quartz: 1-3"),"{note}");
        assert!(note.contains("partial") || note.contains("parcial"));
        assert!(note.len()<600,"no repeated source body or full diagnostic: {note}");
        assert!(!note.contains("restore();"));
        assert_eq!(input.raw["tool_response"],original);
        assert!(context(&input,&ctx).is_none(),"no repeated context for the same agent");
        input.agent_id=Some("worker".into());assert!(context(&input,&ctx).is_some());
        input.tool_input=json!({"file_path":root.path().join("src/main.rs"),"offset":1,"limit":3});
        input.raw["tool_response"]["file"]=json!({"filePath":root.path().join("src/main.rs"),"content":SOURCE,"numLines":3,"startLine":1,"totalLines":3});
        assert!(context(&input,&ctx).is_none(),"an owner read whole needs no note");
    }
    #[test]
    fn a_read_that_holds_every_owner_whole_gains_no_note() {
        let root=indexed();let (mut input,ctx)=read(root.path());
        input.tool_input=json!({"file_path":root.path().join("src/main.rs")});
        input.raw["tool_response"]["file"]=json!({"filePath":root.path().join("src/main.rs"),"content":SOURCE,"numLines":3,"startLine":1,"totalLines":3});
        assert!(context(&input,&ctx).is_none(),"the read already shows every owner whole");
        let (cut,ctx)=read(root.path());
        let note=context(&cut,&ctx).unwrap();
        assert!(note.contains("quartz: 1-3"),"an owner that starts or ends outside the excerpt still goes: {note}");
    }
    #[test]
    fn stale_unknown_partial_and_mismatched_read_outputs_preserve_native_fallback() {
        let root=indexed();let (input,ctx)=read(root.path());
        for (pointer,value) in [
            ("/type",json!("image")),("/truncated",json!(true)),("/isError",json!(true)),
            ("/interrupted",json!(true)),("/file/truncatedByTokenCap",json!(true)),
            ("/file/content",json!("stale();\n")),("/file/numLines",json!(2)),
            ("/file/startLine",json!(1)),("/file/totalLines",json!(1)),
            ("/file/filePath",json!(root.path().join("mustard.json"))),
        ] {
            let mut changed=input.clone();
            let (parent,key)=pointer.rsplit_once('/').unwrap();
            changed.raw["tool_response"].pointer_mut(parent).unwrap()[key]=value;
            assert!(context(&changed,&ctx).is_none(),"{pointer}");
            assert!(matches!(super::super::after_search(&changed,&ctx),mustard_core::domain::model::contract::Verdict::Allow));
        }
        std::fs::write(root.path().join("src/main.rs"),SOURCE.replace("quartz","different")).unwrap();
        assert!(context(&input,&ctx).is_none(),"unchanged line cannot validate an old owner");
        std::fs::remove_file(mustard_core::io::project_map::model_path(root.path())).unwrap();
        assert!(context(&input,&ctx).is_none());
        assert!(!mustard_core::io::project_map::model_path(root.path()).is_file(),"no scan/database is created for a bare Read");
    }
    #[test]
    fn a_map_held_by_another_writer_does_not_hold_the_read() {
        let root=indexed();let (input,ctx)=read(root.path());
        let model=mustard_core::io::project_map::model_path(root.path());
        let (held,holding)=std::sync::mpsc::channel();let (release,released)=std::sync::mpsc::channel::<()>();
        let base=root.path().to_path_buf();
        let writer=std::thread::spawn(move||{
            let mut db=mustard_core::io::map_db::MapDb::open(&model,&base,&[]).unwrap();
            db.write(|_|{held.send(()).unwrap();let _=released.recv();Ok(())}).unwrap();
        });
        holding.recv().unwrap();
        // A leitura volta com a trava ainda presa: só depois ela é solta.
        let note=context(&input,&ctx);
        let _=release.send(());writer.join().unwrap();
        assert!(note.is_some_and(|note|note.contains("quartz: 1-3")),"the owners still come from the map being read");
    }
    #[cfg(unix)]
    #[test]
    fn a_checkout_alias_retains_the_current_owner() {
        let root=indexed();let links=tempfile::tempdir().unwrap();let alias=links.path().join("checkout");
        std::os::unix::fs::symlink(root.path(),&alias).unwrap();
        let (input,ctx)=read(&alias);
        assert!(context(&input,&ctx).unwrap().contains("quartz: 1-3"));
    }
    #[cfg(unix)]
    #[test]
    fn a_read_through_a_symlink_cannot_cross_source_outside_the_project() {
        let root=indexed();let outside=tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("other.rs"),SOURCE).unwrap();
        std::os::unix::fs::symlink(outside.path().join("other.rs"),root.path().join("src/alias.rs")).unwrap();
        let (mut input,ctx)=read(root.path());
        input.tool_input["file_path"]=json!(root.path().join("src/alias.rs"));
        input.raw["tool_response"]["file"]["filePath"]=input.tool_input["file_path"].clone();
        assert!(context(&input,&ctx).is_none());
    }
}
