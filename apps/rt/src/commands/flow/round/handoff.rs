//! A passagem do despacho ao programa recém-compilado. No checkout do
//! Mustard, a rodada que vai despachar refaz o programa compilado da branch
//! quando ele está atrás do commit atual; se é esse mesmo programa que roda a
//! rodada, o processo atual é o antigo, e a onda que ele soltasse sairia
//! montada pelo código de antes. Então, com a compilação verde, a rodada
//! termina a sua parte e pede o despacho ao programa novo (`run round --dispatch --full`, sem
//! relatório), devolvendo a resposta dele junto da sua. A resposta inteira, e
//! não a curta: o processo que imprime é este, e é ele quem a encurta, uma vez
//! só — agrupar as tarefas seguradas de uma resposta já agrupada as perde. Sem
//! compilação, fora do
//! repositório do Mustard, com outro programa rodando a rodada, com onda
//! pausada pelo relatório — que só este processo conhece — ou com o programa
//! novo sem resposta legível, o despacho segue no próprio processo.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Quem roda o programa `program` com os argumentos dados e devolve a
/// resposta dele lida como JSON; `None` quando ele não roda ou não responde
/// JSON. Um teste o troca para provar a regra sem rodar programa nenhum.
pub(super) type Runner<'a> = &'a dyn Fn(&Path, &[String]) -> Option<Value>;

/// O programa compilado da branch do Mustard de `root` quando é ele que roda
/// agora; `None` fora do repositório do Mustard ou com outro programa
/// rodando (o instalado, o de um `cargo test`).
pub(super) fn running_build(root: &Path) -> Option<PathBuf> {
    let main = mustard_core::mustard_checkout(root)?;
    let program = crate::shared::development_build::program_in(&mustard_core::io::wave_prompt::development_build_dir(&main));
    let running = std::env::current_exe().ok()?;
    same_program(&running, &program).then_some(program)
}

/// `true` quando `running` é o arquivo `program`. O Linux diz o programa
/// trocado no disco com ` (deleted)` no fim do caminho, e esse caminho
/// ainda é o dele.
fn same_program(running: &Path, program: &Path) -> bool {
    let shown = running.to_string_lossy();
    let running = Path::new(shown.strip_suffix(" (deleted)").unwrap_or(&shown));
    let resolved = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    resolved(running) == resolved(program)
}

/// A resposta do despacho feito pelo programa novo, quando ele cabe: a
/// rodada refez o programa (`rebuilt`), nenhuma onda pausou por este
/// relatório (`paused`), e o programa refeito é o que roda agora
/// (`program`). O despacho é a rodada da spec `spec` em `start` sem
/// relatório, pelo `run`, com a resposta inteira (`--full`): quem a encurta é
/// o processo que imprime. `None` manda despachar no próprio processo.
pub(super) fn dispatch_by_new_program(start: &Path, spec: &str, rebuilt: bool, paused: bool, program: Option<PathBuf>, run: Runner<'_>) -> Option<Value> {
    if !rebuilt || paused {
        return None;
    }
    let program = program?;
    let args: Vec<String> =
        ["run", "round", "--dispatch", "--full", "--spec", spec, "--root"].into_iter().map(str::to_string).chain([start.display().to_string()]).collect();
    run(&program, &args).filter(|answer| answer.get("ok").is_some())
}

/// A marca do programa novo que recebeu o despacho: ele despacha sem
/// compilar de novo, e nunca passa o despacho adiante. Sem ela, o programa
/// que a compilação não deixou em dia compilaria e passaria o despacho a si
/// mesmo sem fim.
const HANDED_OVER: &str = "MUSTARD_RT_HANDED_OVER";

/// `true` no programa que recebeu o despacho de outra rodada.
pub(super) fn handed_over() -> bool {
    std::env::var_os(HANDED_OVER).is_some()
}

/// Roda o programa, com a marca de quem recebeu o despacho, e lê a última
/// linha JSON da saída dele: a rodada responde numa linha só, e a saída de
/// erro fica de fora.
pub(super) fn run_program(program: &Path, args: &[String]) -> Option<Value> {
    let out = std::process::Command::new(program).args(args).env(HANDED_OVER, "1").output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().rev().find_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
}

/// A resposta da rodada que passou o despacho: a do programa novo, com o
/// que esta rodada gravou, formatou e comitou antes dela, e os avisos das
/// duas, os desta primeiro. O programa novo lê a spec de depois do commit e
/// refaz as conferências das voltas que seguem esperando, como esta rodada
/// acabou de fazer: o aviso que ele repete igual ao daqui sai uma vez só.
pub(super) fn joined(recorded: &[Value], formatted: &[String], commit: Option<&Value>, warnings: &[Value], mut answer: Value) -> Value {
    let theirs = |answer: &Value, field: &str| answer[field].as_array().cloned().unwrap_or_default();
    let all_recorded: Vec<Value> = recorded.iter().cloned().chain(theirs(&answer, "recorded")).collect();
    let all_warnings: Vec<Value> = warnings.iter().cloned().chain(theirs(&answer, "warnings").into_iter().filter(|warning| !warnings.contains(warning))).collect();
    answer["recorded"] = json!(all_recorded);
    if !formatted.is_empty() {
        answer["formatted"] = json!(formatted);
    }
    if let Some(commit) = commit {
        answer["commit"] = commit.clone();
    }
    if !all_warnings.is_empty() {
        answer["warnings"] = json!(all_warnings);
    }
    answer
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// Com o programa refeito e rodando, o despacho vai ao programa novo, com
    /// a rodada sem relatório da mesma spec e raiz, e a resposta dele volta;
    /// sem recompilação, com onda pausada ou com outro programa rodando, o
    /// executor nem é chamado, e o despacho fica no próprio processo.
    #[test]
    fn a_rebuilt_program_takes_the_dispatch_and_without_rebuild_it_stays_here() {
        let calls: RefCell<Vec<(PathBuf, Vec<String>)>> = RefCell::new(Vec::new());
        let run = |program: &Path, args: &[String]| {
            calls.borrow_mut().push((program.to_path_buf(), args.to_vec()));
            Some(json!({"ok": true, "dispatch": [{"wave": 7}]}))
        };
        let program = PathBuf::from("/build/mustard-dev/mustard-rt");
        let start = Path::new("/repo");

        let answer = dispatch_by_new_program(start, "x", true, false, Some(program.clone()), &run).expect("the new program dispatches");
        assert_eq!(answer["dispatch"][0]["wave"], json!(7));
        assert_eq!(
            calls.borrow().as_slice(),
            [(program.clone(), ["run", "round", "--dispatch", "--full", "--spec", "x", "--root", "/repo"].map(str::to_string).to_vec())]
        );

        for (rebuilt, paused, running) in [(false, false, Some(program.clone())), (true, true, Some(program.clone())), (true, false, None)] {
            assert!(dispatch_by_new_program(start, "x", rebuilt, paused, running, &run).is_none());
        }
        assert_eq!(calls.borrow().len(), 1, "only the rebuilt and running program is called");

        let silent = |_: &Path, _: &[String]| Some(json!("not a round answer"));
        assert!(dispatch_by_new_program(start, "x", true, false, Some(program), &silent).is_none(), "an unreadable answer dispatches here");
    }

    /// A resposta juntada leva o que esta rodada gravou e comitou antes do
    /// que o programa novo gravou, e os avisos dos dois.
    #[test]
    fn the_joined_answer_keeps_the_commit_and_both_records() {
        let answer = json!({"ok": true, "recorded": [{"type": "send"}], "warnings": [{"reason": "b"}], "dispatch": [{"wave": 2}]});
        let joined = joined(&[json!({"type": "commit"})], &["a.rs".into()], Some(&json!({"sha": "abc"})), &[json!({"reason": "a"})], answer);
        assert_eq!(joined["recorded"], json!([{"type": "commit"}, {"type": "send"}]));
        assert_eq!(joined["warnings"], json!([{"reason": "a"}, {"reason": "b"}]));
        assert_eq!(joined["commit"]["sha"], json!("abc"));
        assert_eq!(joined["formatted"], json!(["a.rs"]));
        assert_eq!(joined["dispatch"][0]["wave"], json!(2));
    }

    /// O programa trocado no disco ainda é o mesmo, pelo caminho que o
    /// Linux diz dele.
    #[test]
    fn a_program_replaced_on_disk_is_still_the_running_one() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("mustard-rt");
        std::fs::write(&program, "").unwrap();
        assert!(same_program(Path::new(&format!("{} (deleted)", program.display())), &program));
        assert!(!same_program(&dir.path().join("other"), &program));
    }
}
