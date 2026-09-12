//! Section cutting and prompt cleanup: read a subproject's `## Guards`, cut the
//! spec's `## Tasks` (with narrative fallbacks), cut the parent spec's
//! conversation material for one wave ([`build_conversation_material`]), resolve
//! the spec locale, and the two post-substitution passes that keep the rendered
//! prompt clean ([`collapse_empty_sections`],
//! [`strip_unfilled_template_tokens`]).

use super::reference::files_section_paths;
use crate::commands::scan_claude::GUARDS_PENDING_OPEN;
use crate::commands::spec::spec_sections::{is_heading, section_end};
use mustard_core::io::fs as mfs;
use std::fmt::Write as _;
use std::path::Path;

/// Prefixed to a `## Guards` body that is still the `/scan` scaffold.
///
/// The pending block's entire body is HTML comments, so copying it verbatim
/// hands the agent an EMPTY rule set that renders exactly like a curated one —
/// nothing in the dispatched prompt distinguishes "this project has no rules
/// yet" from "these are the rules". This line does.
const UNCURATED_GUARDS_NOTICE: &str = "> NOTE: this subproject's `## Guards` block is still the uncurated `/scan` \
     scaffold — the enrich pass never authored its rules. There are NO project \
     rules below: fall back to the sibling-convention read and do not treat the \
     placeholder as guidance.";

/// Read the `## Guards` section body from a subproject's instruction file. Empty
/// when the file or the section is absent.
///
/// `root` is the project root, and it is not decoration: the file this reads is
/// resolved through [`crate::shared::context::guards_file`], which under a
/// private install prefers the untracked `CLAUDE.local.md` the scan wrote there.
/// This block IS the mechanism the Guards exist for — inlined under `## GUARDS`
/// into every dispatched prompt — so a hard-coded `CLAUDE.md` here means a
/// private install dispatches agents with the CLIENT's rules, or with none.
///
/// When the block still carries [`GUARDS_PENDING_OPEN`] the body is a
/// placeholder, not rules: [`UNCURATED_GUARDS_NOTICE`] is prefixed so the
/// dispatch says so. Fail-open per the inject contract — a missing/unreadable
/// file yields no injection (empty string), and nothing here panics.
pub fn read_guards_block(root: &Path, subproject_dir: &Path) -> String {
    let source = crate::shared::context::guards_file(root, subproject_dir);
    let text = mfs::read_to_string(source).unwrap_or_default();
    if text.is_empty() {
        return String::new();
    }
    // NB: deliberately a bespoke single-pass scan, NOT `section_end` — a
    // subproject `CLAUDE.md` may carry an *indented* `## Guards`, so both the
    // start match and the boundary `trim_start()` first. The shared scanner
    // anchors at column 0; folding it in would change behaviour.
    let mut in_section = false;
    let mut collected = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("## ") {
            if in_section {
                break; // Next `## ` ends the section.
            }
            let after = line.trim_start().trim_start_matches('#').trim();
            if after.eq_ignore_ascii_case("Guards") {
                in_section = true;
                continue;
            }
        }
        if in_section {
            collected.push_str(line);
            collected.push('\n');
        }
    }
    let block = collected.trim().to_string();
    // Absent section ⇒ no injection at all (the empty `## GUARDS` heading is
    // then collapsed away by `collapse_empty_sections`) — never a bare notice.
    if block.is_empty() || !block.contains(GUARDS_PENDING_OPEN) {
        return block;
    }
    format!("{UNCURATED_GUARDS_NOTICE}\n\n{block}")
}

/// Cut the `## Tarefas` / `## Tasks` section from a spec file. Empty when
/// neither heading exists.
///
/// Lean bugfix/Light specs carry no `## Tasks` checklist — their work lives in
/// `## Causa raiz` / `## Plano`. When the structured section is missing or has
/// no body, fall back to [`build_task_fallback`] so the dispatched agent still
/// receives a non-empty TASK block (root cause + plan, or — when those are
/// absent too — a POINTER at the `## WHY` / `## ACCEPTANCE` sections this prompt
/// already carries, plus a read-the-spec cue) instead of a blank one. Full specs
/// are unaffected: a present, non-empty `## Tasks` section is always preferred
/// and returned byte-identical.
/// `why_block` e `acceptance_block` são os corpos JÁ COMPOSTOS das duas seções
/// que este prompt vai carregar ([`build_why_block`] e [`read_wave_acceptance`],
/// vazios quando a seção colapsa). Eles entram porque o tier 2 do fallback é um
/// PONTEIRO, e um ponteiro só pode ser escrito a partir do que está mesmo no
/// prompt — ver [`task_fallback_pointer`].
pub(crate) fn read_task_steps(
    spec_path: &Path,
    why_block: &str,
    acceptance_block: &str,
) -> String {
    let text = mfs::read_to_string(spec_path).unwrap_or_default();
    if text.is_empty() {
        return String::new();
    }
    let structured = cut_tasks_section(&text);
    if !structured.is_empty() {
        return structured;
    }
    build_task_fallback(&text, spec_path, why_block, acceptance_block)
}

/// Extract the `## Tasks` / `## Tarefas` / `## Checklist` section body (heading
/// included). Empty when the heading is absent or carries no content lines.
fn cut_tasks_section(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|l| is_heading(l, "tasks")) else {
        return String::new();
    };
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if l.starts_with("## ") {
            end = i;
            break;
        }
    }
    // Require at least one non-blank body line under the heading; an empty
    // `## Tasks` heading must trigger the fallback, not emit a bare heading.
    let has_body = lines[start + 1..end].iter().any(|l| !l.trim().is_empty());
    if !has_body {
        return String::new();
    }
    lines[start..end].join("\n").trim_end().to_string()
}

/// Build the `## REALITY OBLIGATIONS` body for a wave — the duties the plan
/// declared against the world OUTSIDE the repository (an official document, a
/// live endpoint, a stored row), rendered into the wave's `spec.md` by the
/// wave-scaffold renderer.
///
/// The instruction line is composed HERE rather than sitting statically under
/// the template heading, because a static body would keep the heading alive for
/// every wave that declares no duty — and a section that is present and empty
/// reads as "there are no duties" when it means "this plan never said". Empty
/// when the wave declares none, which collapses the heading like any other.
///
/// Fail-open: an unreadable spec yields "". EN by the agent-prompt policy, like
/// the vocabulary block and `## GIT BOUNDARY`.
pub(crate) fn read_reality_obligations(spec_path: &Path) -> String {
    let text = mfs::read_to_string(spec_path).unwrap_or_default();
    let duties = crate::commands::wave::wave_scaffold::parse_reality_obligations(&text);
    if duties.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "These duties are about the WORLD, not the code: verify each one against the source \
         outside this repository (the official document, the live endpoint, the stored row) \
         BEFORE writing the code it governs. Do NOT infer the answer from the codebase. In \
         your final report, account for each duty BY ITS ID — what you checked and what it \
         said — including an id you could not check and why.",
    );
    out.push('\n');
    for (id, duty) in duties {
        let _ = write!(out, "\n- **{id}** — {duty}");
    }
    out
}

/// Monta o corpo do `## ACCEPTANCE` — os critérios que o QA vai EXECUTAR,
/// literais, com as linhas `Command:` / `Expect:` / `Control:` intactas,
/// filtrados pelo `satisfies:` do frontmatter da onda quando há uma.
///
/// Os critérios são a RÉGUA, e até aqui a onda nunca a via: a união mora no
/// `wave-plan.md` (de onde o QA lê) e o prompt da onda carregava 15 campos, e
/// nenhum deles era um critério. O primeiro remédio COPIOU o subconjunto para
/// o spec da onda, e a cópia era um retrato: o layout congela na aprovação, e
/// um critério que o `ac-amend` reescrevia ou o `ac-add` criava aterrissava no
/// pai e nunca chegava ao prompt de onda nenhuma — o agente re-despachado por
/// um achado da review era justamente quem não via o critério escrito para ele.
///
/// Um prompt é renderizado na hora do despacho, e lê a fonte ATUAL — a MESMA
/// que o juiz lê ([`ruler_source`]): a união do `wave-plan.md`, e o
/// `## Acceptance Criteria` do pai só para a spec que plano de ondas nenhum
/// materializou. Ler o pai aqui discordava do QA em dois casos medidos, e nos
/// dois o agente era julgado por um texto que nunca viu: uma onda com linhas de
/// `acceptance` PRÓPRIAS (o pai não define o id, o bloco colapsava, e a
/// rastreabilidade contava a onda como coberta), e um rewave (o pai vira
/// `spec.original.md`, que o `ac-amend` não reescreve, então o prompt mostrava
/// o comando VELHO e o QA rodava o novo). A seção é escolhida pelo MESMO
/// `section_block` que o `qa-run` usa, então um rascunho legado com o título
/// duplicado rende a lista, não o placeholder. A onda contribui só o filtro
/// ([`wave_scaffold::parse_wave_ruler`]): sem `satisfies` não há régua e o
/// bloco volta vazio; sem onda (render de nível-spec) a seção inteira é a régua
/// da unidade.
///
/// A linha de instrução é composta AQUI em vez de ficar estática sob o título do
/// template, pelo mesmo motivo que [`read_reality_obligations`] compõe a dela:
/// um corpo estático manteria o título vivo para toda onda que não declara
/// critério nenhum, e uma seção presente e vazia se lê como "não há nenhum"
/// quando quer dizer "esta onda não disse". Uma onda de REWAVE carrega a régua
/// inteira e diz isso ([`wave_scaffold::REWAVE_ACCEPTANCE_NOTE`]) antes da
/// lista, para o "JUIZ desta onda" não afirmar uma atribuição que o DAG não fez.
///
/// O título recortado é rebaixado para `###` para aninhar sob o `## ACCEPTANCE`
/// em vez de encerrá-lo — ver [`demote_heading`]. Fail-open: spec ilegível
/// devolve "". O TEXTO da instrução fica em EN, pela política de prompt de
/// agente.
pub(crate) fn read_wave_acceptance(parent_spec: &Path, wave_spec: Option<&Path>) -> String {
    use crate::commands::wave::wave_scaffold::{parse_wave_ruler, REWAVE_ACCEPTANCE_NOTE};
    let Some(section) = ruler_source(parent_spec) else {
        return String::new();
    };
    let section = section.trim_end();
    let (heading, items) = section.split_once('\n').unwrap_or((section, ""));
    let heading = demote_heading(heading);
    let mut parts: Vec<String> = Vec::new();
    match wave_spec {
        None => parts.push(demote_heading(section)),
        Some(wave) => {
            let ruler = parse_wave_ruler(&mfs::read_to_string(wave).unwrap_or_default());
            let mine: Vec<String> = criterion_blocks(items)
                .into_iter()
                .filter(|(id, _)| ruler.satisfies.contains(id))
                .map(|(_, block)| block)
                .collect();
            if mine.is_empty() {
                return String::new();
            }
            parts.push(heading);
            if ruler.carried_whole {
                parts.push(REWAVE_ACCEPTANCE_NOTE.to_string());
            }
            parts.push(mine.join("\n"));
        }
    }
    if parts.iter().all(|p| p.trim().is_empty()) {
        return String::new();
    }
    format!(
        "These criteria are the JUDGE of this wave, not a suggestion: each `Command:` below is \
         run VERBATIM by the QA gate, and its exit code (plus the `Expect:` pattern when one is \
         declared) is the verdict on your work. Read them BEFORE you write code, and make each \
         one pass — do not restate, reinterpret or narrow them.\n\n{}",
        parts.join("\n\n")
    )
}

/// Fatia o corpo de um `## Acceptance Criteria` em UM bloco literal por critério
/// — a linha de cabeçalho mais as suas linhas de continuação (`Command:`,
/// `Expect:`, `Control:`) — na ordem do documento, com o id NORMALIZADO (trim +
/// maiúsculas, a mesma normalização do `satisfies:` que o consulta).
///
/// A fronteira é a MESMA que o lookahead do `parse_ac_items` varre (próximo
/// cabeçalho de AC, linha em branco ou `## `), e o cabeçalho é reconhecido pelo
/// MESMO `parse_ac_header` que o `qa-run` usa — um segundo leitor de linha de
/// critério é um segundo jeito de errar qual comando julga a onda.
fn criterion_blocks(section: &str) -> Vec<(String, String)> {
    use crate::commands::review::qa_run::parse_ac_header;
    let lines: Vec<&str> = section.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((id, _)) = parse_ac_header(lines[i]) else {
            i += 1;
            continue;
        };
        let mut block: Vec<&str> = vec![lines[i].trim_end()];
        let mut j = i + 1;
        while j < lines.len() {
            let line = lines[j];
            if parse_ac_header(line).is_some() || line.trim().is_empty() || line.starts_with("## ") {
                break;
            }
            block.push(line.trim_end());
            j += 1;
        }
        out.push((id.trim().to_uppercase(), block.join("\n")));
        i = j;
    }
    out
}

/// O nome do índice do plano, ao lado do spec do pai — o arquivo de onde o QA
/// lê a união dos critérios.
const WAVE_PLAN_MD: &str = "wave-plan.md";

/// A seção `## Acceptance Criteria` que o JUIZ vai executar, cortada pelo mesmo
/// `section_block` que o `qa-run` usa: a união do `wave-plan.md` quando o plano
/// materializou uma, e a seção do PAI ([`read_parent_spec`]) quando não há plano
/// de ondas — a spec light / tactical-fix, cujo caminho isto não muda.
///
/// UM arquivo para o leitor e para o juiz, por construção. Enquanto a régua saía
/// do pai, dois casos medidos rendiam prompt e veredito de fontes diferentes: a
/// onda com `acceptance` PRÓPRIA (o id só existe na união, então o pai não
/// definia nada e o bloco colapsava — e o portão de rastreabilidade ainda
/// contava a onda como coberta) e o rewave (o pai é renomeado para
/// `spec.original.md`, que o `ac-amend`/`ac-add` não reescreve: o prompt
/// mostrava o comando superado sob "estes são o JUIZ desta onda"). A ordem
/// aqui é a mesma que o `qa_run::find_spec_file` aplica quando o `spec.md`
/// some, e não há um segundo jeito de escolher a régua.
///
/// `None` quando nem a união nem o pai declaram a seção. Fail-open: arquivo
/// ilegível é o mesmo que arquivo ausente.
fn ruler_source(parent_spec: &Path) -> Option<String> {
    use crate::commands::spec::spec_sections::section_block;
    let dir = parent_spec.parent().unwrap_or_else(|| Path::new("."));
    let union = mfs::read_to_string(dir.join(WAVE_PLAN_MD)).unwrap_or_default();
    section_block(&union, "acceptance-criteria")
        .or_else(|| section_block(&read_parent_spec(parent_spec), "acceptance-criteria"))
}

/// O spec do PAI, lido pela regra que o `wave-scaffold` aplica ao mesmo arquivo:
/// `spec.md`, e `spec.original.md` quando aquele não abre. Um rewave RENOMEIA o
/// spec do pai para lá no passo 9 da decomposição, então sem o fallback os dois
/// canais que são MESMO do pai (`## WHY` e `## CONVERSATION MATERIAL`) morrem
/// exatamente para a população que TEM ondas. A régua NÃO é um deles: ela sai da
/// união que o QA executa ([`ruler_source`]), e só cai aqui quando plano de
/// ondas nenhum existe. Fail-open: nada legível devolve "".
fn read_parent_spec(parent_spec: &Path) -> String {
    let archived =
        parent_spec.parent().unwrap_or_else(|| Path::new(".")).join("spec.original.md");
    mfs::read_to_string(parent_spec)
        .or_else(|_| mfs::read_to_string(&archived))
        .unwrap_or_default()
}

/// Monta o corpo do `## WHY` de uma onda despachada — as seções `## Context` e
/// `## Non-Goals` do spec do PAI, recortadas por chave canônica (então
/// `## Contexto` / `## Não-Objetivos` resolvem igual).
///
/// O arquivo da onda carrega O QUÊ e ONDE; o motivo de o trabalho existir, e o
/// terreno que a unidade deliberadamente NÃO cobre, moram uma vez só no pai e
/// não chegavam a onda nenhuma. O único caminho que tinham era o fallback de
/// TASK, que só dispara para um spec sem `## Tasks` — e onda de escopo completo
/// sempre tem uma.
///
/// O título de cada seção recortada é rebaixado para `###` ([`demote_heading`]):
/// uma linha `## ` como primeira linha do corpo encerraria a seção `## WHY` em
/// vez de ficar dentro dela, e o `collapse_empty_sections` então derrubaria o
/// título WHY como vazio deixando o corpo órfão para trás.
///
/// Vazio quando o pai não declara nenhuma das duas, o que colapsa o título.
/// Fail-open: spec ilegível devolve "". O pai é aberto por [`read_parent_spec`]
/// — a mesma resolução (`spec.md` → `spec.original.md`) que o `## CONVERSATION
/// MATERIAL` usa, para os dois canais que são do PAI não discordarem sobre qual
/// arquivo ele é. A régua não passa por aqui: ela sai da união que o QA executa
/// ([`ruler_source`]).
pub(crate) fn build_why_block(parent_spec: &Path) -> String {
    let text = read_parent_spec(parent_spec);
    let mut parts: Vec<String> = Vec::new();
    for key in ["context", "non-goals"] {
        if let Some(body) = cut_section_by_key(&text, key) {
            parts.push(demote_heading(&body));
        }
    }
    parts.join("\n\n")
}

/// Rebaixa o título `## ` que abre uma seção recortada para `### `, deixando
/// toda outra linha em paz.
///
/// A seção recortada chega COM o título dela, e uma linha `## ` dentro do corpo
/// de uma `## SECTION` renderizada não é conteúdo aninhado — é a próxima seção,
/// tanto para quem lê quanto para o [`collapse_empty_sections`]. Um `#` é o
/// conserto inteiro, e ele preserva o título que o autor escreveu (no idioma
/// dele) em vez de trocá-lo por um rótulo fixo em inglês.
fn demote_heading(block: &str) -> String {
    if block.starts_with("## ") {
        format!("#{block}")
    } else {
        block.to_string()
    }
}

/// Build a TASK block from the spec body when no structured `## Tasks` section
/// exists. Tier 1: the `## Causa raiz` / `## Root cause` section (when
/// present) plus the `## Plano` / `## Plan` section. Tier 2 (when tier 1 finds
/// nothing — the tactical-fix / drafted-spec shape): a POINTER naming where the
/// narrative and the ruler already are in this same prompt. Both tiers append an
/// explicit instruction to read the full spec before editing. Empty only when no
/// narrative section is present at all (the renderer then degrades to a blank
/// TASK as before).
///
/// O tier 2 copiava as duas seções, e nenhuma das duas é dele. Os critérios
/// saíram primeiro, porque passaram a ter bloco próprio (`## ACCEPTANCE`,
/// [`read_wave_acceptance`]); o `## Contexto` saiu pelo MESMO argumento, que
/// ninguém tinha aplicado nele: o `## WHY` ([`build_why_block`]) recorta esse
/// mesmo texto do mesmo arquivo, e num render de nível-spec — que é justamente
/// onde este fallback dispara — os dois recortes são do mesmo `spec.md`, então o
/// Contexto saía DUAS vezes no prompt.
///
/// O que sobrou no tier 2 é um PONTEIRO — e um ponteiro só vale enquanto aponta
/// para algo que ESTÁ no prompt. Duas coisas seguem daí, e as duas são regra:
///
/// 1. **Dispara por QUALQUER uma das duas.** Condicionar o tier inteiro ao
///    `## Contexto` deixava indespachável a spec que declara critérios sob um
///    título narrativo diferente (`## Problema`, `## Resumo`): o TASK voltava
///    vazio e o `render::run` RECUSA um `## TASK` vazio com exit 2. Uma spec sem
///    o título canônico é uma spec que degrada, nunca uma spec que não anda.
/// 2. **Nomeia SÓ o que vai ser renderizado.** O `## ACCEPTANCE` colapsa quando
///    a spec não declara critério ([`read_wave_acceptance`] devolve "" e o
///    [`collapse_empty_sections`] derruba o título), e o `## WHY` colapsa quando
///    ela não tem `## Contexto` nem `## Não-Objetivos` ([`build_why_block`]).
///    Apontar para uma seção que não está no prompt é pior que não apontar: o
///    agente vai procurar e não acha.
///
/// A pergunta NÃO é feita ao texto que este fallback tem em mãos: é feita aos
/// BLOCOS que o compositor já montou, e é por isso que eles descem até aqui.
/// Perguntar ao texto local funcionava enquanto os três recortes saíam do mesmo
/// arquivo — o render de nível-spec —, e mentia no render de ONDA: o `## WHY` é
/// recortado do spec do PAI e este texto é o da onda, então uma onda sem
/// `## Contexto` próprio, cujo pai tem um, recebia o `## WHY` renderizado e,
/// logo abaixo dele, a frase dizendo que o prompt não carrega narrativa nenhuma.
///
/// Vazio só quando não há nem narrativa nem régua para apontar: aí não existe
/// ponteiro honesto a dar, e o TASK em branco de sempre é a resposta.
fn build_task_fallback(
    text: &str,
    spec_path: &Path,
    why_block: &str,
    acceptance_block: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(body) = cut_section_by_display(text, &["Root cause", "Causa raiz"]) {
        parts.push(body);
    }
    if let Some(body) = cut_section_by_display(text, &["Plan", "Plano"]) {
        parts.push(body);
    }
    if parts.is_empty() {
        // O ponteiro, não a cópia: a narrativa já viaja no `## WHY` e os
        // critérios no `## ACCEPTANCE`.
        if let Some(pointer) =
            task_fallback_pointer(!why_block.trim().is_empty(), !acceptance_block.trim().is_empty())
        {
            parts.push(pointer);
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    parts.push(format!(
        "Read the full spec at {} before editing.",
        spec_path.display()
    ));
    parts.join("\n\n")
}

/// O ponteiro do tier 2, nomeando SÓ as seções que o prompt vai mesmo carregar.
///
/// `why` e `ruler` NÃO são medidos aqui, e é a correção inteira: eles são a
/// presença dos blocos que o compositor montou ([`build_why_block`] e
/// [`read_wave_acceptance`]), passados por quem os montou. Enquanto esta função
/// os re-media do texto local, ela media o arquivo ERRADO num render de onda — o
/// `## WHY` sai do spec do PAI, e a onda cujo `spec.md` não tem `## Contexto`
/// recebia a seção renderizada com a frase "this prompt does not say why the
/// work exists" logo abaixo dela. Uma pergunta que se responde do mesmo lugar de
/// onde o texto é recortado não tem como discordar dele.
///
/// Sem nenhuma das duas não há para onde apontar, e o retorno é `None` — o TASK
/// em branco de sempre.
///
/// Pura, total; EN pela política de prompt de agente, como todo o resto do
/// bloco.
fn task_fallback_pointer(why: bool, ruler: bool) -> Option<String> {
    let head = "> TASK fallback: the spec has no `## Tasks` section.";
    let tail = "Derive the steps from it.";
    // As frases falam do PROMPT, nunca de "this same spec": num render de onda o
    // `## WHY` vem do spec do PAI e o `## ACCEPTANCE` do da onda, então a
    // procedência que a frase antiga afirmava era falsa metade das vezes. O que o
    // agente precisa saber é onde a seção está — e ela está aqui.
    Some(match (why, ruler) {
        (true, true) => format!(
            "{head} The story of WHY this work exists is in `## WHY` above, and the criteria \
             this work is judged by are in `## ACCEPTANCE` — both already in this prompt. \
             Derive the steps from them."
        ),
        (true, false) => format!(
            "{head} The story of WHY this work exists is in `## WHY` above. Nothing in this \
             prompt states how the work will be judged — no acceptance criteria reached it. \
             {tail}"
        ),
        (false, true) => format!(
            "{head} The criteria this work is judged by are in `## ACCEPTANCE` above. Nothing \
             in this prompt says why the work exists — no narrative section reached it. {tail}"
        ),
        (false, false) => return None,
    })
}

/// Cut a `## <name>` section body (heading included) by literal display name,
/// case-insensitively, matching any of `names`. Used for narrative-divider
/// headings (`## Plan`/`## Plano`) that are intentionally absent from the
/// canonical `SECTIONS` table, so `is_heading` does not resolve them. Returns
/// `None` when the heading is absent or carries no body content.
fn cut_section_by_display(text: &str, names: &[&str]) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| {
        let Some(rest) = l.strip_prefix("##") else {
            return false;
        };
        let after = rest.trim_start_matches([' ', '\t']);
        if after.len() == rest.len() {
            return false; // `## ` requires whitespace after the hashes.
        }
        names.iter().any(|n| after.trim_end().eq_ignore_ascii_case(n))
    })?;
    cut_section_at(&lines, start)
}

/// Cut a `## <key>` section body (heading included) by canonical section key
/// via [`is_heading`] — i18n-aware, so `context` matches both `## Context` and
/// `## Contexto`. Returns `None` when the heading is absent or carries no body
/// content. FIRST heading wins — fine for the narrative sections this serves;
/// the criteria go through `spec_sections::section_block`, whose defensive pick
/// among duplicated headings is the one QA uses.
fn cut_section_by_key(text: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| is_heading(l, key))?;
    cut_section_at(&lines, start)
}

/// The section slice starting at `start` (the heading line, inclusive) through
/// the line before the next `## ` heading or EOF. `None` when the body is
/// entirely blank (an empty heading must not survive into the TASK block).
fn cut_section_at(lines: &[&str], start: usize) -> Option<String> {
    let end = section_end(lines, start);
    let has_body = lines[start + 1..end].iter().any(|l| !l.trim().is_empty());
    if !has_body {
        return None;
    }
    Some(lines[start..end].join("\n").trim_end().to_string())
}

// ---------------------------------------------------------------------------
// The conversation material, cut per wave
// ---------------------------------------------------------------------------

/// EN sub-headings for the carried material — fixed literals, exactly like the
/// drafter's own `## Definitions` / `## Decisions` / `## Evidence`. Agent
/// prompts stay EN by policy (the same rule the vocabulary block and
/// `## GIT BOUNDARY` follow), and one literal per kind keeps every consumer
/// keyed off the same string instead of the spec's localised heading.
const MATERIAL_DEFINITIONS: &str = "### Definitions";
const MATERIAL_DECISIONS: &str = "### Decisions";
const MATERIAL_EVIDENCE: &str = "### Evidence";

/// Cut the parent spec's conversation material (`spec-draft --material`) for ONE
/// wave.
///
/// The material lives ONCE, in `parent_spec`; a per-wave copy would drift, so
/// the cut happens here, at render time. Each kind has a different natural key:
///
/// - **Definitions** are the shared vocabulary — every wave gets them, or each
///   wave invents its own term for the same thing again.
/// - **Decisions** are the law of the work ("everything branches off dev" binds
///   every wave), so they are not cut either.
/// - **Findings** carry a file, so the FILE is the key: a finding reaches only
///   the wave whose declared `## Files` list contains it. That intersection is
///   computed over [`files_section_paths`] — the SAME list the reference-files
///   builder reads — so this cut cannot disagree with the rest of the pipeline.
///
/// `wave_spec` is the wave's operational spec
/// (`resume_bootstrap::resolve_operational_spec_path`); on a wave-less render it
/// IS `parent_spec`, and the cut then runs against the parent's own `## Files`.
///
/// Empty when the spec carries no material, or when nothing survives the cut —
/// the `## CONVERSATION MATERIAL` heading is then dropped by
/// [`collapse_empty_sections`], so a spec that carries nothing renders a prompt
/// byte-identical to one rendered before this channel existed. Fail-open: an
/// unreadable spec yields "".
///
/// The parent is opened by [`read_parent_spec`] — the SAME `spec.md` →
/// `spec.original.md` resolution `## WHY` uses. A rewave archives the parent
/// under that name, and this arm used to read `spec.md` directly: every wave
/// rendered after the archiving carried the story from the archive and an EMPTY
/// material section, so the definitions/decisions/evidence channel died for
/// exactly the population that has waves. (`## ACCEPTANCE` no longer reads the
/// parent at all — its source is the union QA executes, [`ruler_source`].)
///
/// Returns the rendered block AND the [`MaterialCensus`] of what the cut did, so
/// the dispatch can REPORT what it held back instead of printing a bare total
/// that reads as a truncation. The census is computed by the cut ITSELF — a
/// second counter re-reading the parent spec would be a second spelling of this
/// rule, and the two would drift.
pub(crate) fn build_conversation_material(
    parent_spec: &Path,
    wave_spec: &Path,
) -> (String, MaterialCensus) {
    let text = read_parent_spec(parent_spec);
    let wave_text = mfs::read_to_string(wave_spec).unwrap_or_default();
    cut_material_for_files(&text, &files_section_paths(&wave_text))
}

/// [`build_conversation_material`] against text already in hand, with the wave's
/// declared files given directly instead of parsed out of its `spec.md`.
///
/// The wave SCAFFOLD needs exactly this cut while it is still WRITING the wave's
/// `spec.md` — the file whose `## Files` the path-taking face would read does
/// not exist yet. Two cuts would be two rules; this is the one rule, and
/// [`build_conversation_material`] is its file-reading face.
pub(crate) fn cut_material_for_files(
    parent_text: &str,
    declared: &[String],
) -> (String, MaterialCensus) {
    let mut census = MaterialCensus::default();
    if parent_text.is_empty() {
        return (String::new(), census);
    }
    let mut out = String::new();
    for (key, heading) in [
        ("definitions", MATERIAL_DEFINITIONS),
        ("decisions", MATERIAL_DECISIONS),
    ] {
        if let Some(body) = cut_section_body(parent_text, key) {
            census.carried += split_bullet_items(&body).len();
            push_material_block(&mut out, heading, &body);
        }
    }
    if let Some(body) = cut_section_body(parent_text, "evidence") {
        let (kept, other_wave) = keep_findings_for(&body, declared);
        census.other_wave = other_wave;
        if !kept.is_empty() {
            census.carried += split_bullet_items(&kept).len();
            push_material_block(&mut out, MATERIAL_EVIDENCE, &kept);
        }
    }
    (out, census)
}

/// Echo each material item a task CITES directly under that task line.
///
/// A task cites an item by its handle — `[E-3]`, `[K-7]`, `[D-2]` — the ids
/// [`crate::commands::spec::spec_draft`] stamps when it writes the material
/// sections. `material` is the block this wave carries, already cut.
///
/// **Why the item has to travel to the task.** In the rendered prompt the
/// material is one section and the tasks are another, some seventy lines apart.
/// Nothing joined them, so an agent handed 28 items and 13 tasks had to guess
/// which context governed which step. A trap is usually true of exactly one task
/// — "do not depend on the entry type in this guard" — and that is the only
/// place it gets read in time to matter. The item still lives once, in the
/// material section; this is a pointer resolved in place, not a second copy of
/// the channel.
///
/// A citation naming an id the material does not carry is left ALONE: it is the
/// author's text, and silently deleting it would hide a typo that a reader can
/// otherwise see and fix. Returns `task_steps` unchanged when nothing is cited.
pub(crate) fn echo_cited_material(task_steps: &str, material: &str) -> String {
    if task_steps.is_empty() || material.is_empty() {
        return task_steps.to_string();
    }
    let items = material_items_by_id(material);
    if items.is_empty() {
        return task_steps.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    for line in task_steps.lines() {
        out.push(line.to_string());
        let indent = " ".repeat(line.len() - line.trim_start().len() + 2);
        for id in cited_ids(line) {
            if let Some(body) = items.get(&id) {
                for (i, item_line) in body.lines().enumerate() {
                    let prefix = if i == 0 { "↳ " } else { "  " };
                    out.push(format!("{indent}{prefix}{}", item_line.trim()));
                }
            }
        }
    }
    out.join("\n")
}

/// Index the `[D-n]` / `[K-n]` / `[E-n]` items of a rendered material block by
/// their id. The body keeps the item's own continuation lines (the `Reason:` /
/// `Evidence:` attribute), minus the leading bullet and the handle itself.
fn material_items_by_id(material: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for item in split_bullet_items(material) {
        let Some(first) = item.first() else { continue };
        let Some((id, rest)) = split_handle(first.trim_start_matches("- ")) else {
            continue;
        };
        let mut body = vec![rest.trim().to_string()];
        body.extend(item.iter().skip(1).map(|l| l.trim().to_string()));
        map.insert(id, body.join("\n"));
    }
    map
}

/// Split a leading `[X-n] ` handle off an item line: `("E-3", "the rest")`.
fn split_handle(line: &str) -> Option<(String, &str)> {
    let rest = line.trim_start().strip_prefix('[')?;
    let close = rest.find(']')?;
    let id = rest[..close].trim().to_string();
    is_material_id(&id).then(|| (id, &rest[close + 1..]))
}

/// Every `[X-n]` handle a task line cites, in order, without duplicates.
fn cited_ids(line: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for (open, _) in line.match_indices('[') {
        let rest = &line[open + 1..];
        let Some(close) = rest.find(']') else { continue };
        let id = rest[..close].trim().to_string();
        if is_material_id(&id) && !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// `true` for a well-formed material handle: one of `D` / `K` / `E`, a hyphen,
/// then digits. Nothing else is a handle — a markdown checkbox (`[ ]`, `[x]`) or
/// a link label must never be read as one.
fn is_material_id(id: &str) -> bool {
    let Some((kind, num)) = id.split_once('-') else {
        return false;
    };
    matches!(kind, "D" | "K" | "E") && !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit())
}

/// What the per-wave cut did to the parent spec's conversation material.
///
/// It exists because a bare `material=28` on a spec holding 35 items reads as a
/// SIZE CAP that ate the difference — the operator measured exactly that and
/// concluded the renderer truncates. There is no cap (see the compositor's "No
/// size budget" note). The difference is the per-wave cut doing its job, and a
/// count that cannot say so teaches a defect that does not exist while hiding
/// the one that did: findings with no path, silently dropped from every wave.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MaterialCensus {
    /// Items riding in THIS wave's prompt.
    pub carried: usize,
    /// Findings held back because their evidence file is declared by ANOTHER
    /// wave's `## Files` — the cut working as designed, now reported as such.
    pub other_wave: usize,
}

/// The body of a `## <key>` section, heading EXCLUDED and trimmed. `None` when
/// the heading is absent or its body is entirely blank — an empty kind must
/// contribute no sub-heading at all.
fn cut_section_body(text: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| is_heading(l, key))?;
    let end = section_end(&lines, start);
    let body = lines[start + 1..end].join("\n").trim().to_string();
    if body.is_empty() {
        None
    } else {
        Some(body)
    }
}

/// Append one `### <kind>` block, separated from the previous one by a blank
/// line. The first block never gets a leading separator, so an all-but-one-kind
/// material renders without a stray blank head.
fn push_material_block(out: &mut String, heading: &str, body: &str) {
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(heading);
    out.push('\n');
    out.push_str(body);
}

/// Keep only the findings whose evidence file is in `declared`.
///
/// A finding is one top-level `- ` bullet plus its indented continuation lines
/// (the "bullet + attribute line" shape the drafter emits). The evidence path is
/// the first backtick-quoted token on a CONTINUATION line: the drafter writes
/// exactly one such attribute (`  Evidence: \`file:line\``), and keying off the
/// token rather than the `Evidence:` label keeps a hand-authored PT spec
/// (`Evidência:`) resolving through this same cut instead of growing a second
/// parser. Skipping the bullet line itself means a backtick inside the statement
/// is never mistaken for the path.
///
/// A finding with no evidence path is UNATTRIBUTABLE, and it rides EVERYWHERE.
///
/// It used to be dropped, on the reasoning that it belongs to no wave. Measured
/// in the field: that reasoning silently deleted it from ALL of them. A trap the
/// operator wrote as "do not depend on the entry type in this guard", with no
/// `file:line` beside it, reached no agent at all — while the parent spec still
/// showed it, so nothing looked lost. A channel whose whole purpose is carrying
/// what the conversation settled may not answer silence; an unattributable
/// finding is treated like a decision (it binds every wave) instead.
///
/// A wave declaring NO `## Files` is the same case one level up: nothing can be
/// attributed against an empty list, so everything rides rather than nothing.
///
/// Returns the kept block plus the number of findings held back for another
/// wave — the count the dispatch reports, so a cut is never mistaken for a cap.
fn keep_findings_for(body: &str, declared: &[String]) -> (String, usize) {
    let mut kept: Vec<String> = Vec::new();
    let mut other_wave = 0usize;
    for item in split_bullet_items(body) {
        let rendered = item.join("\n").trim_end().to_string();
        match item.iter().skip(1).find_map(|l| backtick_path(l)) {
            // No path, or no boundary to check it against — carry it.
            None => kept.push(rendered),
            _ if declared.is_empty() => kept.push(rendered),
            Some(path) if declared.iter().any(|d| same_file(d, &path)) => kept.push(rendered),
            Some(_) => other_wave += 1,
        }
    }
    (kept.join("\n"), other_wave)
}

/// Group a bullet list into items: each top-level `- ` line plus every following
/// line until the next top-level bullet. Lines before the first bullet are
/// discarded (a kind's body is a bullet list by construction).
fn split_bullet_items(body: &str) -> Vec<Vec<&str>> {
    let mut items: Vec<Vec<&str>> = Vec::new();
    for line in body.lines() {
        if line.starts_with("- ") {
            items.push(vec![line]);
        } else if let Some(last) = items.last_mut() {
            last.push(line);
        }
    }
    items
}

/// The first backtick-quoted token on a line, with a trailing `:<line>` suffix
/// removed — `` `src/a.rs:12` `` and `` `src/a.rs` `` both yield `src/a.rs`, so
/// a line-precise finding and a file-level one key the same way.
fn backtick_path(line: &str) -> Option<String> {
    let open = line.find('`')?;
    let rest = &line[open + 1..];
    let close = rest.find('`')?;
    let inner = rest[..close].trim();
    if inner.is_empty() {
        return None;
    }
    let path = match inner.rsplit_once(':') {
        Some((head, tail))
            if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) =>
        {
            head
        }
        _ => inner,
    };
    Some(path.to_string())
}

/// Whether two path spellings name the same file.
///
/// A `## Files` entry is subproject-relative OR repo-relative (the reference
/// builder resolves both spellings), while a finding records the path it was
/// READ at. Plain string equality would therefore cut everything away on a
/// monorepo. Segment-anchored suffix containment is the deterministic relation
/// that accepts both spellings without guessing a root — and staying anchored on
/// `/` keeps `foo/bar.rs` from matching `notfoo/bar.rs`.
fn same_file(a: &str, b: &str) -> bool {
    let a = normalise_path(a);
    let b = normalise_path(b);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.ends_with(&format!("/{b}")) || b.ends_with(&format!("/{a}"))
}

/// Normalise a path spelling for comparison: Windows separators to `/`, no
/// leading `./`, no surrounding slashes. Case is left alone — the repo's paths
/// are case-sensitive on the platforms that matter for the census.
fn normalise_path(p: &str) -> String {
    p.trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string()
}

/// Filter the lines of a task block by a regex-style pattern.
///
/// The heading line (e.g. `## Tarefas`) is always kept. Every subsequent
/// top-level bullet is kept only when its content matches `pattern`.
/// Sub-bullets / blank continuation lines follow the parent bullet's fate.
///
/// Pattern support: literal characters + `\\.` escape + `(a|b|c)` alternation.
/// This covers the common `T0\\.(1|5)` dispatch-slicing use case without
/// pulling in a full regex crate. Patterns that cannot be parsed warn on
/// stderr and leave the block unfiltered.
pub(crate) fn filter_task_lines(raw: &str, pattern: &str) -> String {
    // Expand the pattern into one or more literal alternatives so that
    // `T0\.(1|5)` becomes ["T0.1", "T0.5"].
    let alternatives = expand_pattern(pattern);

    let mut out: Vec<&str> = Vec::new();
    let mut keep_continuation = false;
    for line in raw.lines() {
        // Section headings are always kept.
        if line.starts_with("## ") || line.starts_with("# ") {
            out.push(line);
            keep_continuation = false;
            continue;
        }
        // Top-level bullet (not indented).
        if line.starts_with("- ") {
            // Strip `- [ ] ` / `- [x] ` / `- ` prefix to reach the content.
            let content = line
                .trim_start_matches('-')
                .trim_start()
                .trim_start_matches(['[', 'x', ' ', ']'])
                .trim_start();
            keep_continuation = alternatives.iter().any(|alt| content.contains(alt.as_str()));
            if keep_continuation {
                out.push(line);
            }
        } else {
            // Blank lines and continuation/sub-bullet lines follow parent.
            if keep_continuation {
                out.push(line);
            }
        }
    }
    out.join("\n")
}

/// Expand a simplified pattern into a set of literal strings to match against.
///
/// Rules applied in order:
/// 1. `\\.` → literal `.` (unescape).
/// 2. `(a|b|c)` → cross-product with the prefix/suffix around the group.
/// 3. All other characters are kept as-is.
///
/// If the pattern contains unsupported constructs (nested groups, `*`, `+`,
/// `?`, `^`, `$`, character classes `[...]`), the function logs a warning and
/// returns the raw pattern as a single alternative (substring match fallback).
fn expand_pattern(pattern: &str) -> Vec<String> {
    // Detect unsupported constructs (anything beyond `\.` and `(a|b)`).
    let unsupported = pattern
        .chars()
        .any(|c| matches!(c, '*' | '+' | '?' | '^' | '$' | '[' | ']'));
    if unsupported {
        eprintln!(
            "agent-prompt-render: WARN: --task-filter pattern '{pattern}' \
             contains unsupported regex construct — using as literal substring"
        );
        return vec![pattern.to_string()];
    }

    // Unescape `\.` → `.` first, then expand one `(a|b|c)` group if present.
    let unescaped = pattern.replace("\\.", ".");
    match unescaped.find('(') {
        None => vec![unescaped],
        Some(open) => {
            let close = unescaped[open..].find(')').map(|i| open + i);
            let Some(close) = close else {
                eprintln!(
                    "agent-prompt-render: WARN: --task-filter pattern '{pattern}' \
                     has unmatched '(' — using as literal substring"
                );
                return vec![unescaped];
            };
            let prefix = &unescaped[..open];
            let suffix = &unescaped[close + 1..];
            let inner = &unescaped[open + 1..close];
            inner
                .split('|')
                .map(|alt| format!("{prefix}{alt}{suffix}"))
                .collect()
        }
    }
}

/// Find unfilled `{placeholder}` tokens (lowercase + underscore identifiers).
/// Returns each token once, in the order encountered.
pub(crate) fn scan_unfilled(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Find closing `}` without whitespace inside (placeholders never
            // contain whitespace — code-fence blocks like `{ foo }` are ignored).
            let mut j = i + 1;
            let mut all_id = true;
            while j < bytes.len() && bytes[j] != b'}' {
                let c = bytes[j];
                if !(c.is_ascii_lowercase() || c == b'_' || c.is_ascii_digit()) {
                    all_id = false;
                    break;
                }
                j += 1;
            }
            if all_id && j < bytes.len() && j > i + 1 {
                let token = &text[i..=j];
                let owned = token.to_string();
                if !out.contains(&owned) {
                    out.push(owned);
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Blank every **template** placeholder left unfilled after substitution, and
/// return the list that was blanked (for the WARN log).
///
/// `template_tokens` is the set of `{token}`s present in the *original* template
/// block, captured before substitution. A `{token}` in `rendered` that is NOT in
/// that set arrived through substituted spec content — e.g. a literal `{entity}`
/// an author wrote in the wave's `## Tasks` line. That is the author's text, not
/// a render gap: it is left verbatim, never warned-on and never stripped (the
/// old behaviour stripped *any* `{token}`, silently corrupting the task body and
/// emitting a spurious `unfilled placeholder {entity}` warning).
pub(crate) fn strip_unfilled_template_tokens(
    rendered: &str,
    template_tokens: &std::collections::HashSet<String>,
) -> (String, Vec<String>) {
    let mut out = rendered.to_string();
    let mut unfilled = Vec::new();
    for token in scan_unfilled(rendered) {
        if template_tokens.contains(&token) {
            out = out.replace(&token, "");
            unfilled.push(token);
        }
    }
    (out, unfilled)
}

/// Remove any `## ` heading whose body — every line until the next `## ` heading
/// or end of text — is entirely whitespace. Keeps the dispatched prompt clean
/// when a fail-open placeholder (`{guards_summary}`, `{context_md}`,
/// `{reference_files}`, `{cross_wave_memory}`, `{prior_wave_diff}`) resolves to
/// "". Only `## `-level headings are considered, so the `<!-- PREFIX-STABLE -->`
/// marker and inline prose are never touched. The `## TASK` section always
/// survives: its trailing "Guards carregados …" line is non-blank body.
pub(crate) fn collapse_empty_sections(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("## ") {
            let mut j = i + 1;
            while j < lines.len() && !lines[j].starts_with("## ") {
                j += 1;
            }
            if lines[i + 1..j].iter().all(|l| l.trim().is_empty()) {
                i = j; // Drop the heading and its blank body.
                continue;
            }
        }
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    /// O que o COMPOSITOR faz num render de nível-spec: os dois blocos são
    /// montados primeiro, do mesmo arquivo, e o TASK é lido com eles em mãos —
    /// o ponteiro do tier 2 nunca se pergunta nada por conta própria. Ver a
    /// ordem em `render::render_prompt_at`.
    fn task_steps_of(path: &Path) -> String {
        read_task_steps(path, &build_why_block(path), &read_wave_acceptance(path, None))
    }

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scan_unfilled_finds_typed_tokens() {
        let text = "hello {foo} and {bar_baz} {already} { skip } not_a_placeholder";
        let tokens = scan_unfilled(text);
        assert_eq!(tokens, vec!["{foo}", "{bar_baz}", "{already}"]);
    }

    #[test]
    fn scan_unfilled_ignores_whitespace_braces() {
        // Code-fence-style `{ ... }` blocks (with whitespace) are not placeholders.
        let text = "fn f() { let x = 1; }";
        assert!(scan_unfilled(text).is_empty());
    }

    /// Regression (#4): only placeholders the TEMPLATE declared are stripped /
    /// warned. A `{entity}` that arrived via substituted spec content (e.g. a
    /// literal `{entity}` in the wave's `## Tasks`) must survive verbatim — the
    /// old code stripped any `{token}`, corrupting the task body and emitting a
    /// spurious `unfilled placeholder {entity}` warning.
    #[test]
    fn strip_unfilled_only_touches_template_tokens() {
        let template_tokens: std::collections::HashSet<String> =
            ["{foo}".to_string()].into_iter().collect();
        // `{foo}` is a genuine unfilled template placeholder → stripped + warned.
        // `{entity}` is author content from the task body → left verbatim.
        let rendered = "task: implement {entity} now\nleftover {foo} here";
        let (out, unfilled) = strip_unfilled_template_tokens(rendered, &template_tokens);
        assert!(out.contains("{entity}"), "author token must survive: {out}");
        assert!(!out.contains("{foo}"), "unfilled template token must be stripped: {out}");
        assert_eq!(unfilled, vec!["{foo}".to_string()]);
    }

    #[test]
    fn read_guards_block_extracts_section() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("CLAUDE.md"),
            "# Title\n\n## What\n- foo\n\n## Guards\n- rule A\n- rule B\n\n## Stack\nrust\n",
        )
        .unwrap();
        // Root == subproject dir: these cases are about SECTION CUTTING, and a
        // tree with no repository resolves as a shared install, so the resolver
        // names `CLAUDE.md` — the file each case wrote.
        let guards = read_guards_block(dir.path(), dir.path());
        assert!(guards.contains("rule A"));
        assert!(guards.contains("rule B"));
        assert!(!guards.contains("Stack"));
        assert!(
            !guards.contains("uncurated"),
            "a curated block must be copied verbatim: {guards}"
        );
    }

    /// A `## Guards` block still carrying the pending sentinel is a PLACEHOLDER
    /// (its whole body is HTML comments). The dispatch must say so, and the
    /// placeholder itself must survive — the notice is a prefix, not a swap.
    #[test]
    fn read_guards_block_marks_the_pending_scaffold() {
        use crate::commands::scan_claude::GUARDS_CLOSE;
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("CLAUDE.md"),
            format!(
                "# Sub\n\n## Guards\n\n{GUARDS_PENDING_OPEN}\n\
                 <!-- facts: kind=cargo; frameworks=serde -->\n{GUARDS_CLOSE}\n\n## Stack\nrust\n"
            ),
        )
        .unwrap();
        let guards = read_guards_block(dir.path(), dir.path());
        assert!(
            guards.starts_with("> NOTE:"),
            "the notice must lead the block, before the placeholder: {guards}"
        );
        assert!(guards.contains("NO project rules"), "{guards}");
        assert!(guards.contains(GUARDS_PENDING_OPEN), "the raw block must survive: {guards}");
    }

    /// Fail-open (inject contract): no `CLAUDE.md`, or one with no `## Guards`
    /// section, yields NO injection — never a bare notice on an empty body.
    #[test]
    fn read_guards_block_missing_source_yields_no_injection() {
        let dir = tempdir().unwrap();
        assert_eq!(read_guards_block(dir.path(), dir.path()), "", "absent file ⇒ no injection");
        std::fs::write(dir.path().join("CLAUDE.md"), "# Sub\n\n## Stack\nrust\n").unwrap();
        assert_eq!(read_guards_block(dir.path(), dir.path()), "", "absent section ⇒ no injection");
    }

    #[test]
    fn collapse_empty_sections_drops_blank_keeps_filled() {
        let text = "## A\n\n## B\nbody\n\n## C\n   \n## D\nx";
        let out = collapse_empty_sections(text);
        assert!(!out.contains("## A"), "empty heading A survived: {out}");
        assert!(out.contains("## B\nbody"), "filled heading B dropped: {out}");
        assert!(!out.contains("## C"), "whitespace-only heading C survived: {out}");
        assert!(out.contains("## D\nx"), "filled heading D dropped: {out}");
    }

    // --- build_conversation_material -----------------------------------------

    /// Write a parent spec carrying all three kinds and a wave spec declaring
    /// `files`; returns the pair of paths the cut runs over.
    fn material_fixture(dir: &Path, files: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let parent = dir.join("spec.md");
        std::fs::write(
            &parent,
            "# T\n\n## Definitions\n\n- **wave** — one level of the plan\n\n\
             ## Decisions\n\n- everything branches off dev\n  Reason: the release train\n\n\
             ## Evidence\n\n- alpha parses twice\n  Evidence: `apps/rt/src/alpha.rs:12`\n\
             - beta swallows the error\n  Evidence: `src/beta.rs`\n",
        )
        .unwrap();
        let wave_dir = dir.join("wave-1-x");
        std::fs::create_dir_all(&wave_dir).unwrap();
        let wave = wave_dir.join("spec.md");
        std::fs::write(&wave, format!("# W\n\n## Files\n\n{files}\n## Tasks\n\n- [ ] x\n")).unwrap();
        (parent, wave)
    }

    /// The finding cut is a set intersection over the wave's `## Files`, and a
    /// subproject-relative `## Files` entry still matches a repo-relative
    /// evidence path (the two spellings both occur in a monorepo).
    #[test]
    fn conversation_material_cuts_findings_by_declared_files() {
        let dir = tempdir().unwrap();
        let (parent, wave) = material_fixture(dir.path(), "- `src/alpha.rs`\n");
        let (out, census) = build_conversation_material(&parent, &wave);
        assert!(out.contains("### Definitions"), "{out}");
        assert!(out.contains("### Decisions"), "{out}");
        assert!(out.contains("### Evidence"), "{out}");
        assert!(out.contains("alpha parses twice"), "declared file's finding missing: {out}");
        assert!(!out.contains("beta swallows"), "undeclared file's finding leaked: {out}");
        // The census says what the cut did, in both directions: one definition,
        // one decision and one finding rode; one finding stayed behind for the
        // wave that declares its file. That second number is the whole point —
        // a bare total reads as a truncation.
        assert_eq!(census.carried, 3, "{census:?}");
        assert_eq!(census.other_wave, 1, "{census:?}");
    }

    /// A finding with NO evidence path rides to EVERY wave.
    ///
    /// It used to be dropped from all of them, which deleted it while the parent
    /// spec still displayed it — the silent loss the operator measured as a
    /// truncating size cap.
    #[test]
    fn a_finding_without_a_path_reaches_every_wave() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("spec.md");
        std::fs::write(
            &parent,
            "# T\n\n## Evidence\n\n- do not depend on the entry type in this guard\n\
             - alpha parses twice\n  Evidence: `src/alpha.rs:12`\n",
        )
        .unwrap();
        let wave_dir = dir.path().join("wave-1-x");
        std::fs::create_dir_all(&wave_dir).unwrap();
        let wave = wave_dir.join("spec.md");
        std::fs::write(&wave, "# W\n\n## Files\n\n- `src/other.rs`\n").unwrap();

        let (out, census) = build_conversation_material(&parent, &wave);
        assert!(
            out.contains("do not depend on the entry type"),
            "the unattributable trap must ride: {out}"
        );
        assert!(!out.contains("alpha parses twice"), "another wave's finding leaked: {out}");
        assert_eq!(census.carried, 1, "{census:?}");
        assert_eq!(census.other_wave, 1, "{census:?}");
    }

    /// A wave declaring no `## Files` has no boundary to cut against, so it gets
    /// everything rather than nothing.
    #[test]
    fn a_wave_with_no_declared_files_carries_every_finding() {
        let dir = tempdir().unwrap();
        let (parent, _) = material_fixture(dir.path(), "- `src/alpha.rs`\n");
        let bare = dir.path().join("bare.md");
        std::fs::write(&bare, "# W\n\n## Tasks\n\n- [ ] x\n").unwrap();
        let (out, census) = build_conversation_material(&parent, &bare);
        assert!(out.contains("alpha parses twice"), "{out}");
        assert!(out.contains("beta swallows"), "{out}");
        assert_eq!(census.other_wave, 0, "nothing is held back with no boundary: {census:?}");
    }

    /// A task citing `[E-1]` gets that item echoed under it; a citation of an id
    /// the material does not carry is left alone (the author's own text).
    #[test]
    fn a_task_citing_an_item_gets_it_echoed_underneath() {
        let material = "### Evidence\n- [E-1] the query evaporates under soft delete\n  \
                        Evidence: `src/q.rs:10`\n- [E-2] unrelated\n  Evidence: `src/z.rs`\n";
        let tasks = "## Tasks\n- [ ] guard the deletion path [E-1]\n- [ ] unrelated work [E-9]\n";
        let out = echo_cited_material(tasks, material);

        let lines: Vec<&str> = out.lines().collect();
        let at = lines
            .iter()
            .position(|l| l.contains("guard the deletion path"))
            .expect("task line kept");
        assert!(
            lines[at + 1].contains("↳ the query evaporates under soft delete"),
            "the cited item must sit right under its task: {out}"
        );
        assert!(lines[at + 2].contains("src/q.rs:10"), "the item's own evidence rides: {out}");
        // The uncited item is not echoed anywhere.
        assert!(!out.contains("unrelated\n"), "{out}");
        // An unknown handle survives verbatim — a typo stays visible.
        assert!(out.contains("[E-9]"), "{out}");
        // A markdown checkbox is never read as a handle.
        assert!(out.contains("- [ ] guard the deletion path"), "{out}");
    }

    /// A wave that declares NONE of the evidence files gets the vocabulary and
    /// the law of the work, and no `### Evidence` sub-heading at all.
    #[test]
    fn conversation_material_drops_evidence_heading_when_nothing_matches() {
        let dir = tempdir().unwrap();
        let (parent, wave) = material_fixture(dir.path(), "- `src/gamma.rs`\n");
        let (out, census) = build_conversation_material(&parent, &wave);
        assert!(out.contains("### Definitions"), "{out}");
        assert!(!out.contains("### Evidence"), "empty evidence heading survived: {out}");
        assert!(!out.contains("alpha parses twice"), "{out}");
        assert_eq!(census.other_wave, 2, "both findings belong elsewhere: {census:?}");
    }

    /// A spec with no material at all yields "" — the caller's heading then
    /// collapses and the prompt is byte-identical to one without the channel.
    #[test]
    fn conversation_material_empty_for_a_spec_without_the_channel() {
        let dir = tempdir().unwrap();
        let parent = dir.path().join("spec.md");
        std::fs::write(&parent, "# T\n\n## Files\n\n- `a.rs`\n\n## Tasks\n\n- [ ] x\n").unwrap();
        assert_eq!(build_conversation_material(&parent, &parent).0, "");
        // Missing file → fail-open, never a panic.
        let missing = dir.path().join("nope.md");
        assert_eq!(build_conversation_material(&missing, &missing).0, "");
    }

    #[test]
    fn backtick_path_strips_the_line_suffix_only() {
        assert_eq!(backtick_path("  Evidence: `src/a.rs:12`").as_deref(), Some("src/a.rs"));
        assert_eq!(backtick_path("  Evidence: `src/a.rs`").as_deref(), Some("src/a.rs"));
        // A non-numeric tail is part of the path, not a line number.
        assert_eq!(backtick_path("  Evidência: `a:b.rs`").as_deref(), Some("a:b.rs"));
        assert_eq!(backtick_path("  no backticks here"), None);
    }

    #[test]
    fn same_file_is_segment_anchored() {
        assert!(same_file("apps/rt/src/a.rs", "src/a.rs"));
        assert!(same_file("src/a.rs", "./apps/rt/src/a.rs"));
        assert!(same_file("src\\a.rs", "src/a.rs"));
        // A shared tail that is not a path segment must NOT match.
        assert!(!same_file("notsrc/a.rs", "rc/a.rs"));
        assert!(!same_file("src/a.rs", "src/b.rs"));
    }

    #[test]
    fn read_task_steps_cuts_section() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n## Resumo\nx\n## Tarefas\n- [ ] do a\n- [ ] do b\n## Deps\nz\n",
        )
        .unwrap();
        let steps = task_steps_of(&path);
        assert!(steps.contains("Tarefas"));
        assert!(steps.contains("do a"));
        assert!(!steps.contains("Deps"));
    }

    #[test]
    fn read_task_steps_falls_back_to_body_when_no_tasks_section() {
        // A lean bugfix/Light spec: no `## Tasks`, but `## Causa raiz` +
        // `## Plano` carry the work. The TASK block must be non-empty so the
        // dispatched agent receives a real work description, plus the explicit
        // read-the-spec instruction line.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n## Contexto\nbrief\n## Causa raiz\nrace on shutdown\n## Plano\n\
             - fix the lock ordering\n## Critérios de Aceitação\n- repro exits 0\n",
        )
        .unwrap();
        let steps = task_steps_of(&path);
        assert!(!steps.is_empty(), "TASK block must not be empty for a lean spec");
        assert!(steps.contains("race on shutdown"), "root cause missing: {steps}");
        assert!(steps.contains("fix the lock ordering"), "plan missing: {steps}");
        assert!(
            steps.contains("Read the full spec at"),
            "read-the-spec instruction missing: {steps}"
        );
    }

    #[test]
    fn task_fallback_tf_without_tasks_yields_context_only() {
        // A tactical-fix / drafted spec: no `## Tasks`, no `## Causa raiz` /
        // `## Plano` — only `## Contexto` + `## Critérios de Aceitação`. The
        // TASK block must be non-empty and POINT at where the narrative and the
        // ruler are, instead of degrading to a blank TASK.
        //
        // Nem os critérios nem o Contexto entram aqui: cada um tem bloco próprio
        // no prompt (`## ACCEPTANCE`, `read_wave_acceptance`; `## WHY`,
        // `build_why_block`), recortado deste mesmo spec — copiá-los aqui
        // imprimiria os dois duas vezes.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# TF\n## Contexto\nthe digest misses pt intents\n\
             ## Critérios de Aceitação\n- **AC-1** — repro query returns hits\n",
        )
        .unwrap();
        let steps = task_steps_of(&path);
        assert!(!steps.is_empty(), "TASK must not be empty for a TF spec");
        assert!(steps.contains("TASK fallback"), "origin header missing: {steps}");
        assert!(steps.contains("`## WHY`"), "the pointer must name the story: {steps}");
        assert!(steps.contains("`## ACCEPTANCE`"), "the pointer must name the ruler: {steps}");
        assert!(
            !steps.contains("the digest misses pt intents"),
            "the context must not be copied in too: {steps}"
        );
        assert!(!steps.contains("AC-1"), "the criteria must not ride in TASK too: {steps}");
        assert!(steps.contains("Read the full spec at"), "read-the-spec cue missing: {steps}");

        // ...and both DO reach the prompt, each through its own channel.
        let acceptance = read_wave_acceptance(&path, None);
        assert!(acceptance.contains("AC-1"), "criteria lost entirely: {acceptance}");
        assert!(acceptance.contains("JUDGE"), "the ruler must name itself: {acceptance}");
        assert!(
            build_why_block(&path).contains("the digest misses pt intents"),
            "the story must still reach the prompt through `## WHY`"
        );

        // Sem narrativa NEM régua não há para onde apontar, e o bloco volta a
        // ser vazio — o TASK em branco de sempre, nunca um ponteiro para o nada.
        let bare = dir.path().join("bare.md");
        std::fs::write(&bare, "# TF\n## Limites\nIN: nada\n").unwrap();
        assert!(task_steps_of(&bare).is_empty(), "{:?}", task_steps_of(&bare));
    }

    /// A REGRESSÃO que este teste tranca: uma spec que declara critérios sob um
    /// título narrativo que não é `## Contexto` voltava com `## TASK` VAZIO, e
    /// `render::run` recusa um TASK vazio com exit 2 — a spec ficava
    /// indespachável em vez de degradar.
    ///
    /// E o ponteiro nomeia SÓ o que o prompt carrega: sem `## Contexto` nem
    /// `## Não-Objetivos` o `## WHY` colapsa, então mandar o agente lê-lo seria
    /// mandá-lo procurar uma seção que não está ali.
    #[test]
    fn task_fallback_fires_on_criteria_alone_and_points_only_at_what_renders() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# TF\n## Problema\no digest perde intents pt\n\
             ## Critérios de Aceitação\n- **AC-1** — repro query returns hits\n",
        )
        .unwrap();
        let steps = task_steps_of(&path);
        assert!(!steps.is_empty(), "a spec ficou indespachável: TASK vazio");
        assert!(steps.contains("`## ACCEPTANCE`"), "a régua tem de ser nomeada: {steps}");
        assert!(
            !steps.contains("`## WHY`"),
            "o `## WHY` colapsa nesta spec — apontar para ele manda procurar o que não existe: \
             {steps}"
        );
        assert!(steps.contains("Read the full spec at"), "read-the-spec cue missing: {steps}");
        // E a outra metade da mesma regra: o bloco que ele nomeia é o que
        // realmente renderiza, e o que ele NÃO nomeia é o que colapsa.
        assert!(read_wave_acceptance(&path, None).contains("AC-1"), "a régua renderiza");
        assert!(build_why_block(&path).is_empty(), "e o `## WHY` colapsa mesmo");
    }

    /// O espelho: narrativa sem régua. O `## ACCEPTANCE` colapsa, então o
    /// ponteiro fala só do `## WHY`.
    #[test]
    fn task_fallback_points_only_at_why_when_the_spec_declares_no_criteria() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(&path, "# TF\n## Contexto\na história inteira\n").unwrap();
        let steps = task_steps_of(&path);
        assert!(steps.contains("`## WHY`"), "a história tem de ser nomeada: {steps}");
        assert!(
            !steps.contains("`## ACCEPTANCE`"),
            "sem critério declarado o `## ACCEPTANCE` colapsa: {steps}"
        );
        assert!(read_wave_acceptance(&path, None).is_empty(), "e ele colapsa mesmo");

        // `## Não-Objetivos` sozinho também arma o `## WHY`, pela mesma regra
        // que o [`build_why_block`] usa — as duas seções, não só a primeira.
        let ng = dir.path().join("ng.md");
        std::fs::write(&ng, "# TF\n## Não-Objetivos\n- não mexer no portão\n").unwrap();
        let steps = task_steps_of(&ng);
        assert!(steps.contains("`## WHY`"), "o não-objetivo também é história: {steps}");
        assert!(!build_why_block(&ng).is_empty(), "e o bloco renderiza mesmo");
    }

    #[test]
    fn task_fallback_matches_en_headings_too() {
        // The canonical-key cut is i18n-aware: an EN-authored spec with
        // `## Context` + `## Acceptance Criteria` resolves the same tier.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# TF\n## Context\nwidget cache is stale\n\
             ## Acceptance Criteria\n- **AC-1** — cache invalidates on write\n",
        )
        .unwrap();
        let steps = task_steps_of(&path);
        // O `## Context` em EN resolve pela mesma chave canônica — é ele que
        // arma o tier 2, mesmo agora que o tier 2 aponta em vez de copiar.
        assert!(steps.contains("TASK fallback"), "the EN heading must arm tier 2: {steps}");
        assert!(!steps.contains("widget cache is stale"), "context copied into TASK: {steps}");
        // The EN `## Acceptance Criteria` resolves through the same canonical
        // key — for the ACCEPTANCE block now, not for the TASK fallback.
        assert!(!steps.contains("cache invalidates on write"), "AC leaked into TASK: {steps}");
        assert!(
            read_wave_acceptance(&path, None).contains("cache invalidates on write"),
            "the EN heading must resolve for the ruler too"
        );
        assert!(
            build_why_block(&path).contains("widget cache is stale"),
            "and the EN `## Context` must resolve for the story too"
        );
    }

    #[test]
    fn task_fallback_spec_with_tasks_stays_byte_identical() {
        // A spec WITH `## Tasks` keeps the exact structured cut — no fallback
        // header, no Context/AC leakage, byte-identical to the section slice.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# T\n## Contexto\nctx prose\n## Tasks\n- [ ] do the thing\n\
             ## Critérios de Aceitação\n- **AC-1** — gate passes\n",
        )
        .unwrap();
        let steps = task_steps_of(&path);
        assert_eq!(steps, "## Tasks\n- [ ] do the thing", "structured cut must be byte-identical");
        assert!(!steps.contains("TASK fallback"), "fallback header leaked: {steps}");
    }

    #[test]
    fn task_fallback_root_cause_tier_still_wins_over_context_tier() {
        // Tier order is stable: when `## Causa raiz`/`## Plano` exist, the
        // Context/AC tier (and its origin header) must NOT engage.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# T\n## Contexto\nctx prose\n## Causa raiz\nrace on shutdown\n\
             ## Plano\n- fix lock order\n## Critérios de Aceitação\n- repro exits 0\n",
        )
        .unwrap();
        let steps = task_steps_of(&path);
        assert!(steps.contains("race on shutdown"));
        assert!(steps.contains("fix lock order"));
        assert!(!steps.contains("TASK fallback"), "tier-2 header leaked: {steps}");
        assert!(!steps.contains("ctx prose"), "tier-2 content leaked: {steps}");
    }

    #[test]
    fn read_task_steps_prefers_structured_tasks_over_fallback() {
        // When `## Tasks` is present and non-empty, no fallback content leaks in.
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "# T\n## Causa raiz\nthe cause\n## Plano\nthe plan\n## Tasks\n- [ ] do the thing\n",
        )
        .unwrap();
        let steps = task_steps_of(&path);
        assert!(steps.contains("do the thing"));
        assert!(!steps.contains("Read the full spec at"), "fallback leaked: {steps}");
        assert!(!steps.contains("the cause"), "root cause leaked: {steps}");
    }
}
