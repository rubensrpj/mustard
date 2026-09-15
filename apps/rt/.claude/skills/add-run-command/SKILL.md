---
name: add-run-command
description: Use quando for preciso adicionar um comando `mustard-rt run <nome>` novo, seja um passo do fluxo ou um comando de apoio, com os quatro registros, a recusa nos dois idiomas e os testes.
---

# Adicionar um comando `run`

Um comando `run` recebe tudo por argumento (`clap`), nunca pelo stdin, imprime JSON e sai com exit 1 quando recusa. O molde são o `read` e o `write` do arquivo de eventos da spec (`apps/rt/src/commands/spec_events/`): a regra mora no núcleo (`packages/core`), e o comando só lê os argumentos, resolve o projeto, chama o núcleo e imprime.

## Passos

1. **A regra, no núcleo.** O que é puro vai em `packages/core/src/domain/<assunto>.rs`, sem disco e sem relógio; o disco vai em `packages/core/src/io/<assunto>.rs`. As recusas são um enum com `reason()`, a razão curta e estável em kebab-case, e `message(lang)`, a mensagem exata no idioma pedido. Veja `Refusal` em `packages/core/src/domain/spec_events.rs`.
2. **O texto, no catálogo** (`packages/core/src/platform/i18n.rs`). Uma chave por recusa, com o texto exato em pt-BR e en-US e as vagas `{x}` que o chamador troca. O teste do catálogo do assunto lista as chaves com as vagas, como `i18n_translates_spec_event_keys`.
3. **O comando, na família** (`apps/rt/src/commands/<família>/`). Um arquivo por comando, com:
   - `pub struct <Nome>Opts`;
   - uma função testável `pub(crate) fn <nome>_at(opts) -> Value`, ou `-> Result<String, Value>`, que nunca entra em pânico;
   - `pub fn run(opts)`, que imprime e sai com exit 1 na recusa.
   O projeto e o idioma vêm de um lugar só (`project(start)` em `spec_events/mod.rs`), e a recusa sai por `refused(&refusal, lang)`: `{"ok": false, "reason": …, "hint": …}`.
4. **Os quatro registros.** Esquecer qualquer um compila, e o comando some ou a verificação reprova:
   - a variante no enum da família, em `apps/rt/src/commands/<família>/cli.rs`, com `#[command(display_order = N)]`, em que N é o maior de hoje mais um (`grep -rh 'display_order = ' apps/rt/src/commands | sort -t= -k2 -n | tail -1`);
   - o braço no `dispatch()` do mesmo arquivo;
   - o nome, em ordem alfabética, na lista de `apps/rt/tests/run_command_surface.rs`, e a contagem no comentário acima dela;
   - um chamador na prosa do produto (`plugin/**`, `apps/cli/templates/**`) ou uma linha justificada no `RUNTIME_WHITELIST` de `apps/rt/tests/template_parity.rs`, em ordem alfabética. Quando a prosa passar a chamar o comando, a linha sai: o teste reprova a linha que sobra.
   Família nova: `pub mod <família>;` e a variante `#[command(flatten)]` em `apps/rt/src/commands/mod.rs`, com o braço no `dispatch()` de lá.
5. **As opções.** Toda opção longa, como `--spec`, precisa aparecer escrita em algum texto do produto; senão, uma linha justificada no `FLAG_WHITELIST` de `apps/rt/tests/template_parity.rs`.
6. **Os testes.** Da regra, no núcleo. Do comando, pela função `_at` num `tempdir`. Quando o comportamento depende de processo de verdade, como uma trava entre dois processos ou o exit code, pelo binário em `apps/rt/tests/<assunto>_cli.rs`.

## Exemplo completo

A variante e o braço (`apps/rt/src/commands/spec_events/cli.rs`):

```rust
#[derive(Debug, Subcommand)]
pub enum SpecEventsCmd {
    /// Read ONE block of a spec's event file, never the whole file.
    #[command(display_order = 102)]
    Read {
        /// The block to read, e.g. `state` or `wave-2`.
        block: String,
        /// The spec whose file is read.
        #[arg(long)]
        spec: String,
        /// Keep only the events whose words match this term.
        #[arg(long)]
        term: Option<String>,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    // …
}

pub fn dispatch(cmd: SpecEventsCmd) {
    match cmd {
        SpecEventsCmd::Read { block, spec, term, root } => {
            spec_events::read::run(&spec_events::read::ReadOpts { root, spec, block, term });
        }
        // …
    }
}
```

O núcleo testável do comando (`apps/rt/src/commands/spec_events/write.rs`):

```rust
pub(crate) fn write_at(opts: &WriteOpts) -> Value {
    let project = super::project(&opts.root);
    let lang = project.lang;
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);

    let draft = match serde_json::from_str::<Value>(&opts.json) {
        Ok(Value::Object(map)) => map,
        Ok(other) => {
            let shown: String = other.to_string().chars().take(80).collect();
            return refuse(Refusal::NotAnObject { detail: shown });
        }
        Err(e) => return refuse(Refusal::NotAnObject { detail: e.to_string() }),
    };
    let event_type = opts.event_type.trim();
    // A lição vai para o banco de lições, e o `--spec` é opcional só nela.
    if event_type == LESSON {
        return write_lesson(&project, opts.spec.as_deref(), draft);
    }
    let Some(spec) = opts.spec.as_deref() else {
        return refuse(if type_spec(event_type).is_some() {
            Refusal::SpecRequired { event_type: event_type.to_string() }
        } else {
            Refusal::UnknownType { found: event_type.to_string() }
        });
    };
    let path = match store::spec_file(&project.root, spec) {
        Ok(path) => path,
        Err(refusal) => return refuse(refusal),
    };
    let roots = store::citation_roots(&opts.root, &project.root);
    // A página, o `.md` e a linha da spec no índice são refeitos antes de a
    // trava soltar.
    let mut pages = None;
    let written = store::write_then(&path, event_type, draft, &roots, |log| {
        pages = Some(super::pages::rebuild(&project.root, spec, log, lang));
    });
    match written {
        Ok(written) => json!({ "ok": true, "spec": spec.trim(), "id": written.id, "type": event_type }),
        // (o comando real também devolve `code`, `removed`, `purged` e os avisos)
        Err(refusal) => refuse(refusal),
    }
}

pub fn run(opts: &WriteOpts) {
    let report = write_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}
```

## O teste a escrever

Do comando, pela função (`apps/rt/src/commands/spec_events/write.rs`):

```rust
#[test]
fn an_unknown_type_and_a_missing_field_are_refused_by_name() {
    let dir = tempdir().unwrap();
    let unknown = write(dir.path(), "licao", r#"{"text":"x"}"#);
    assert_eq!(unknown["reason"], json!("unknown-type"));
    assert!(unknown["hint"].as_str().unwrap().contains("licao"));
}
```

Pelo binário (`apps/rt/tests/spec_events_cli.rs`):

```rust
fn rt(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mustard-rt"));
    cmd.arg("run").args(args).arg("--root").arg(root).current_dir(root);
    cmd
}

let unknown = rt(root, &["write", "licao", "--spec", "teste", "--json", "{}"]).output().expect("run");
assert_eq!(unknown.status.code(), Some(1));
```

## Armadilhas

- **Saída estável.** A mesma entrada dá os mesmos bytes: sem hora e sem caminho da máquina na saída. O `json!` muda a ordem das chaves conforme o que a compilação juntou; quando a ordem importa, monte a linha à mão, como `render_line` em `domain::spec_events`.
- **Caminho relativo.** O `--root` chega como `.`, e um `.` não sobe pelas pastas de cima. Passe por `std::path::absolute` antes, como `io::spec_events::spec_root`.
- **Worktree.** O estado do Mustard mora no checkout principal. Resolva a raiz por `mustard_core::io::spec_events::spec_root` ou `mustard_core::io::workspace::linked_worktree_main`, nunca pela pasta atual.
- **Recusa não toca no disco.** Confira tudo antes de abrir o arquivo para escrever; o teste compara os bytes antes e depois da recusa.
- **Nunca falhar.** `unwrap` e `expect` são proibidos fora de teste pelo Clippy; um erro de disco vira recusa com `reason`.
- **Windows.** A verificação roda também no Windows: nos testes, nada de caminho ou comando só de Unix. Use `Path::join`, `tempfile` e o binário por `CARGO_BIN_EXE_mustard-rt`.
- **Comentário em palavras.** Comentário e teste descrevem o comportamento; nunca citam o código de uma regra ou de um critério da spec.

## Exemplos usados

- `apps/rt/src/commands/spec_events/cli.rs`
- `apps/rt/src/commands/spec_events/write.rs`
- `apps/rt/src/commands/spec_events/read.rs`
- `apps/rt/tests/spec_events_cli.rs`
- `packages/core/src/domain/spec_events.rs`
