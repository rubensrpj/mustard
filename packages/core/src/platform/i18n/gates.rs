//! Os portões: o de escrita, a testemunha da aprovação e da mudança, a branch
//! de trabalho e a base, a trava de comandos, os defeitos de clareza do fim da
//! resposta e os rótulos do antigo portão de regressão.
//!
//! Uma parte do catálogo de textos: quem lê chama `translate`, a porta do
//! catálogo, e nunca esta parte direto. Chave nova com um começo que esta
//! parte ainda não responde entra também em `PREFIXES`.

use super::Locale;

/// Os começos de chave (o trecho antes do primeiro ponto) que esta parte
/// responde. Nenhum deles é de outra parte.
pub(super) const PREFIXES: &[&str] = &["write_gate", "approval", "change", "workbranch", "base", "command_guard", "clarity", "gate"];

/// O texto de `key` em `lang`, ou `None` quando a chave não está aqui.
pub(super) fn text(key: &str, lang: Locale) -> Option<&'static str> {
    Some(match (key, lang) {
        // Regression gate verdict labels + messages. These are
        // MACHINE / log strings (gate verdicts consumed by the orchestrator and
        // written to telemetry), so they are ENGLISH regardless of the user's
        // configured locale — only `gate.askuser.*` below stays config-lang.
        ("gate.verdict.green.label", _) => "Green",
        ("gate.verdict.amber.label", _) => "Amber",
        ("gate.verdict.red.label", _) => "Red",
        ("gate.verdict.green.message", _) => "No regression signals.",
        ("gate.verdict.amber.message", _) => "Ambiguous signals detected. Confirmation required.",
        ("gate.verdict.red.message", _) => "Regression detected. Consolidation blocked.",

        // Gate signal layer labels (MACHINE / log, English
        // regardless of locale). Use the `{slot}` placeholders to let callers
        // interpolate the matched term, function name, etc.
        ("gate.signal.vocabulary", _) => "Vocabulary matched: {term} (layer {layer})",
        ("gate.signal.stub", _) => "Stub pattern: {pattern} in {function}",
        ("gate.signal.snapshot", _) => "Function {function} emptied ({before_lines} → {after_lines} lines)",

        // Amber AskUserQuestion (printed as JSON, consumed by orchestrator).
        ("gate.askuser.amber.question", Locale::PtBr) => "O gate detectou sinais ambíguos. Autorizar a consolidação?",
        ("gate.askuser.amber.question", Locale::EnUs) => "The gate detected ambiguous signals. Authorize consolidation?",
        ("gate.askuser.amber.option_authorize", Locale::PtBr) => "Autorizar",
        ("gate.askuser.amber.option_authorize", Locale::EnUs) => "Authorize",
        ("gate.askuser.amber.option_block", Locale::PtBr) => "Bloquear",
        ("gate.askuser.amber.option_block", Locale::EnUs) => "Block",
        ("gate.askuser.amber.option_block_desc", Locale::PtBr) => "Bloqueia a consolidação até resolução.",
        ("gate.askuser.amber.option_block_desc", Locale::EnUs) => "Block consolidation until resolved.",

        // Span-level review (subagent_inject + agent_prompt_render).
        // Vocabulary inject block surfaced in the child agent's prompt so the
        // child knows which terms the gate's Moment 1 scan flags.
        ("gate.vocabulary.inject.heading", Locale::PtBr) => "Vocabulário de regressão",
        ("gate.vocabulary.inject.heading", Locale::EnUs) => "Regression vocabulary",
        ("gate.vocabulary.inject.lead", Locale::PtBr) => {
            "Termos que o gate vai checar no seu plano e diff. Evite usar como justificativa."
        }
        ("gate.vocabulary.inject.lead", Locale::EnUs) => {
            "Terms the gate checks in your plan and diff. Avoid using them as justification."
        }
        ("gate.vocabulary.inject.semantic", Locale::PtBr) => "Semântico (alto)",
        ("gate.vocabulary.inject.semantic", Locale::EnUs) => "Semantic (high)",
        ("gate.vocabulary.inject.pattern", Locale::PtBr) => "Padrão (médio)",
        ("gate.vocabulary.inject.pattern", Locale::EnUs) => "Pattern (medium)",
        // Consolidation block message surfaced when a red verdict closes the wave.
        ("gate.consolidation.blocked", Locale::PtBr) => {
            "Consolidação bloqueada: filho {child} retornou verdict vermelho — {message}"
        }
        ("gate.consolidation.blocked", Locale::EnUs) => {
            "Consolidation blocked: child {child} returned a red verdict — {message}"
        }

        // Work-branch REFUSAL — the checkout holds another unit's branch with
        // uncommitted files, so cutting the second unit here would carry them
        // off. Said by the `spec-draft` cut, in the project's language.
        // `{current}`/`{target}`/`{paths}`/`{more}` are interpolated by
        // `work_branch::BusyCheckout::reason`.
        //
        // `{paths}` names the operator's own work — what WOULD ride along. The
        // census the tool itself wrote has sentences of its own below
        // (`workbranch.busy.census_*`): the remedy differs, so the sentence
        // does.
        ("workbranch.busy.refusal", Locale::PtBr) => {
            "O checkout está na branch '{current}', de OUTRA unidade de trabalho, com trabalho \
             NÃO commitado em: {paths}{more}. Criar '{target}' aqui levaria essas edições junto, \
             para dentro de outra unidade. Commite ou guarde (`git stash`) esse trabalho antes de \
             abrir a segunda unidade."
        }
        ("workbranch.busy.refusal", Locale::EnUs) => {
            "The checkout is on branch '{current}', which belongs to ANOTHER work unit, with \
             UNCOMMITTED work in: {paths}{more}. Cutting '{target}' here would carry those edits \
             along into a different unit. Commit or stash (`git stash`) that work before opening \
             the second unit."
        }

        // The SAME refusal when the probe could not answer at all (`git status`
        // failed, or answered in a shape the parser does not understand). It is
        // a distinct sentence because there are no paths to name, and rendering
        // the one above with an empty list would print "work in: ." — which
        // teaches the operator that the refusal is noise. `{current}`/`{target}`
        // are interpolated by `work_branch::BusyCheckout::reason`.
        ("workbranch.busy.unmeasured", Locale::PtBr) => {
            "O checkout está na branch '{current}', de OUTRA unidade de trabalho, e NÃO consegui \
             medir o que há de não commitado ali (o `git status` não respondeu). Criar '{target}' \
             aqui levaria junto qualquer trabalho pendente, para dentro de outra unidade. Commite \
             ou guarde (`git stash`) o que houver — ou conserte o estado do git — antes de abrir a \
             segunda unidade."
        }
        ("workbranch.busy.unmeasured", Locale::EnUs) => {
            "The checkout is on branch '{current}', which belongs to ANOTHER work unit, and the \
             uncommitted work there could NOT be measured (`git status` did not answer). Cutting \
             '{target}' here would carry whatever is pending along into a different unit. Commit \
             or stash (`git stash`) whatever is there — or repair the git state — before opening \
             the second unit."
        }

        // The base this move is about TRAILS its remote and could not be
        // fast-forwarded — a diverged base, one checked out elsewhere, a dirty
        // file in the way that is the operator's. Nothing is cut: a unit cut
        // from a stale base re-does merged work. Git's own words travel in
        // `{error}`; `{base}` is interpolated by
        // `work_branch::BusyCheckout::reason`.
        ("workbranch.busy.base_stale", Locale::PtBr) => {
            "A base '{base}' está atrás de origin/{base} e não pôde ser avançada — git disse: \
             {error}. Nada foi cortado nem gravado: uma unidade cortada de uma base velha refaz \
             trabalho já integrado. Coloque '{base}' em dia (`git pull --ff-only origin {base}` \
             parado nela; se ela divergiu, resolva a divergência primeiro) e tente de novo."
        }
        ("workbranch.busy.base_stale", Locale::EnUs) => {
            "The base '{base}' is behind origin/{base} and could not be advanced — git said: \
             {error}. Nothing was cut and nothing recorded: a unit cut from a stale base re-does \
             work that is already merged. Bring '{base}' up to date (`git pull --ff-only origin \
             {base}` while on it; if it has diverged, resolve that first) and try again."
        }

        // The base trails its remote, the advance IS a fast-forward, and the
        // only thing in its way is the OPERATOR's uncommitted work in files
        // origin also changed. The census beside it is the tool's and is set
        // aside on its own; their files are named, and the remedy is the stash
        // that unblocks the advance — a `git pull` would fail on the very same
        // files. `{base}`/`{paths}`/`{more}` are interpolated by
        // `work_branch::BusyCheckout::reason`.
        ("workbranch.busy.base_blocked", Locale::PtBr) => {
            "A base '{base}' está atrás de origin/{base}, e avançá-la sobrescreveria trabalho \
             NÃO commitado seu em: {paths}{more}. Nada foi cortado nem gravado, e nada foi \
             tocado. Guarde esse trabalho (`git stash push -- <caminhos>`), coloque '{base}' em \
             dia (`git pull --ff-only origin {base}`), traga-o de volta (`git stash pop`) e \
             tente de novo."
        }
        ("workbranch.busy.base_blocked", Locale::EnUs) => {
            "The base '{base}' is behind origin/{base}, and advancing it would overwrite \
             UNCOMMITTED work of yours in: {paths}{more}. Nothing was cut, nothing recorded, \
             and nothing touched. Stash that work (`git stash push -- <paths>`), bring '{base}' \
             up to date (`git pull --ff-only origin {base}`), take it back (`git stash pop`) \
             and try again."
        }

        // Work-branch BASE UNKNOWN — an emergency unit whose base nothing ever
        // recorded, in a project declaring several it could have been cut from.
        // Nothing is cut, and the operator is told: the harness used to take the
        // outermost candidate and mention it on stderr, which a PreToolUse hook
        // says to nobody (it exits 0). `{target}`/`{candidates}` are
        // interpolated by the `spec-draft` cut.
        ("workbranch.base.unknown", Locale::PtBr) => {
            "Não dá para saber de qual base '{target}' deve sair: este projeto declara várias \
             candidatas ({candidates}) e nada registrou a escolha, então a branch NÃO foi criada. \
             Reabra a unidade com a base explícita (--base) — chutar aqui aponta o trabalho para \
             uma base que ninguém escolheu."
        }
        ("workbranch.base.unknown", Locale::EnUs) => {
            "There is no telling which base '{target}' should be cut from: this project declares \
             several candidates ({candidates}) and nothing recorded the choice, so the branch was \
             NOT created. Re-open the unit with an explicit base (--base) — guessing here aims the \
             work at a base nobody chose."
        }

        // The write gate: one message per rule, in the language of
        // `language.text`. The slots between braces are filled by the gate.
        ("write_gate.secret", Locale::PtBr) => {
            "[Mustard] Arquivo sensível: {file} não pode ser lido nem escrito. Casou com {pattern}."
        }
        ("write_gate.secret", Locale::EnUs) => {
            "[Mustard] Sensitive file: {file} cannot be read or written. It matched {pattern}."
        }
        ("write_gate.spec_file", Locale::PtBr) => {
            "[Mustard] {file} é gravado só pelo binário. Grave o evento com \
             `mustard-rt run write <tipo> --spec {spec}`."
        }
        ("write_gate.spec_file", Locale::EnUs) => {
            "[Mustard] Only the binary writes {file}. Record the event with \
             `mustard-rt run write <type> --spec {spec}`."
        }
        ("write_gate.not_approved", Locale::PtBr) => {
            "[Mustard] A spec {spec} ainda não foi aprovada, e {file} é código do projeto. O código \
             só muda depois de o usuário escolher \"Aprovar\" na pergunta."
        }
        ("write_gate.not_approved", Locale::EnUs) => {
            "[Mustard] The spec {spec} is not approved yet, and {file} is project code. Code \
             changes only after the user chooses \"Approve\" in the question."
        }
        ("write_gate.on_base", Locale::PtBr) => {
            "[Mustard] Você está na branch de integração {branch}, declarada no `git.flow` do \
             `mustard.json`. O Mustard não edita direto numa base: abra a spec numa branch de \
             trabalho antes de editar."
        }
        ("write_gate.on_base", Locale::EnUs) => {
            "[Mustard] You are on the integration branch {branch}, declared in the `git.flow` of \
             `mustard.json`. Mustard never edits a base directly: open the spec on a work branch \
             before editing."
        }
        ("write_gate.unreadable_config", Locale::PtBr) => {
            "[Mustard] O `mustard.json` existe e não se lê, então ninguém sabe quais são as bases \
             de integração e {file} não pode ser escrito. Conserte o `mustard.json` — é o único \
             arquivo que passa enquanto ele não voltar a se ler."
        }
        ("write_gate.unreadable_config", Locale::EnUs) => {
            "[Mustard] The `mustard.json` is there and does not load, so nobody knows which \
             branches are integration bases and {file} cannot be written. Fix `mustard.json` — it \
             is the only file that passes until it reads again."
        }
        ("write_gate.other_branch", Locale::PtBr) => {
            "[Mustard] A spec {spec} mora na branch {branch}, e esta edição está na {current}."
        }
        ("write_gate.other_branch", Locale::EnUs) => {
            "[Mustard] The spec {spec} lives on the branch {branch}, and this edit is on {current}."
        }

        // The approval witness: what it tells the assistant after recording
        // the approval, or when nothing was recorded.
        ("approval.witness.clear", Locale::PtBr) => {
            "[Mustard] O usuário aprovou a spec {spec}. Sugira limpar a conversa com `/clear`: a \
             execução começa numa janela limpa, e a retomada lê o estado da spec."
        }
        ("approval.witness.clear", Locale::EnUs) => {
            "[Mustard] The user approved the spec {spec}. Suggest clearing the conversation with \
             `/clear`: execution starts in a clean window, and resuming reads the spec state."
        }
        ("approval.witness.free_text", Locale::PtBr) => {
            "[Mustard] A spec {spec} espera aprovação, e nada foi gravado: a resposta {selected} \
             não é uma das opções oferecidas. Texto livre nunca aprova. Opções oferecidas: \
             {offered}. Para aprovar, responda de novo escolhendo a opção."
        }
        ("approval.witness.free_text", Locale::EnUs) => {
            "[Mustard] The spec {spec} awaits approval, and nothing was recorded: the answer \
             {selected} is not one of the offered options. Free text never approves. Offered \
             options: {offered}. To approve, answer again by picking the option."
        }
        ("approval.witness.not_affirmative", Locale::PtBr) => {
            "[Mustard] A spec {spec} espera aprovação, e nada foi gravado: a opção escolhida \
             {selected} não é \"Aprovar\". Se era uma recusa, está tudo certo."
        }
        ("approval.witness.not_affirmative", Locale::EnUs) => {
            "[Mustard] The spec {spec} awaits approval, and nothing was recorded: the chosen \
             option {selected} is not \"Approve\". If it was a refusal, nothing is wrong."
        }
        ("approval.witness.no_plan", Locale::PtBr) => {
            "[Mustard] Uma aprovação foi escolhida, e nada foi gravado: nenhuma spec desta sessão \
             está na fase de plano."
        }
        ("approval.witness.no_plan", Locale::EnUs) => {
            "[Mustard] An approval was chosen, and nothing was recorded: no spec of this session \
             is in the plan phase."
        }
        ("approval.question", Locale::PtBr) => "Aprovar esta spec?",
        ("approval.question", Locale::EnUs) => "Approve this spec?",
        ("approval.option", Locale::PtBr) => "Aprovar",
        ("approval.option", Locale::EnUs) => "Approve",
        ("approval.witness.unmet", Locale::PtBr) => {
            "[Mustard] A spec {spec} ainda não pode ser aprovada, e nada foi gravado. Resolva o que \
             falta e faça a pergunta de novo:\n{unmet}"
        }
        ("approval.witness.unmet", Locale::EnUs) => {
            "[Mustard] The spec {spec} cannot be approved yet, and nothing was recorded. Fix what \
             is missing and ask the question again:\n{unmet}"
        }
        ("approval.witness.already", Locale::PtBr) => {
            "[Mustard] A spec {spec} já estava aprovada; nada a gravar."
        }
        ("approval.witness.already", Locale::EnUs) => {
            "[Mustard] The spec {spec} was already approved; nothing to record."
        }

        // O gesto da mudança que parte de um agente: a pergunta, as duas
        // opções e o que a testemunha diz depois do clique.
        ("change.question", Locale::PtBr) => "Aceitar a mudança {code}?",
        ("change.question", Locale::EnUs) => "Accept the change {code}?",
        ("change.accept", Locale::PtBr) => "Aceitar",
        ("change.accept", Locale::EnUs) => "Accept",
        ("change.decline", Locale::PtBr) => "Recusar",
        ("change.decline", Locale::EnUs) => "Decline",
        ("change.witness.accepted", Locale::PtBr) => {
            "[Mustard] O usuário aceitou a mudança {code}. Repita a rodada com o mesmo relatório."
        }
        ("change.witness.accepted", Locale::EnUs) => {
            "[Mustard] The user accepted the change {code}. Run the round again with the same report."
        }
        ("change.witness.declined", Locale::PtBr) => {
            "[Mustard] O usuário recusou a mudança {code}: a rodada não segue com ela. Combine com \
             o usuário o que fazer com a onda."
        }
        ("change.witness.declined", Locale::EnUs) => {
            "[Mustard] The user declined the change {code}: the round does not go on with it. \
             Agree with the user on what to do with the wave."
        }
        ("change.witness.free_text", Locale::PtBr) => {
            "[Mustard] Nada foi aceito para a mudança {code}: a resposta {selected} não é uma das \
             opções oferecidas ({offered}). Texto livre nunca aceita; faça a pergunta de novo."
        }
        ("change.witness.free_text", Locale::EnUs) => {
            "[Mustard] Nothing was accepted for the change {code}: the answer {selected} is not \
             one of the offered options ({offered}). Free text never accepts; ask again."
        }
        ("change.witness.no_spec", Locale::PtBr) => {
            "[Mustard] Nada foi gravado para a mudança {code}: não há spec atual nesta sessão."
        }
        ("change.witness.no_spec", Locale::EnUs) => {
            "[Mustard] Nothing was recorded for the change {code}: this session has no current spec."
        }

        // Recusas da trava de comandos (`apps/rt/src/hooks/bash/safety.rs` e
        // `windows_redirect.rs`). A recusa geral recebe o motivo de uma das
        // chaves seguintes; as vagas vêm do chamador.
        ("command_guard.deny", Locale::PtBr) => {
            "Comando barrado: {reason}. Isso apaga trabalho sem volta.\nComando: {command}\nSe for \
             isso mesmo, peça ao usuário para rodar o comando no terminal dele."
        }
        ("command_guard.deny", Locale::EnUs) => {
            "Command blocked: {reason}. This destroys work with no way back.\nCommand: {command}\nIf \
             this is really what you want, ask the user to run the command in their own terminal."
        }
        ("command_guard.rm_recursive_force", Locale::PtBr) => "apagar pasta à força (`rm` com `-r` e `-f`)",
        ("command_guard.rm_recursive_force", Locale::EnUs) => {
            "deleting a folder by force (`rm` with `-r` and `-f`)"
        }
        ("command_guard.force_push", Locale::PtBr) => {
            "forçar o envio ao servidor (`git push --force`); `--force-with-lease` continua liberado"
        }
        ("command_guard.force_push", Locale::EnUs) => {
            "force-pushing to the server (`git push --force`); `--force-with-lease` is still allowed"
        }
        ("command_guard.reset_hard", Locale::PtBr) => "descartar as mudanças com `git reset --hard`",
        ("command_guard.reset_hard", Locale::EnUs) => "discarding changes with `git reset --hard`",
        ("command_guard.clean_force", Locale::PtBr) => {
            "apagar os arquivos que estão fora do git com `git clean -f`"
        }
        ("command_guard.clean_force", Locale::EnUs) => "deleting untracked files with `git clean -f`",
        ("command_guard.checkout_all", Locale::PtBr) => {
            "descartar todas as mudanças com `git checkout -- .`"
        }
        ("command_guard.checkout_all", Locale::EnUs) => "discarding every change with `git checkout -- .`",
        ("command_guard.restore_all", Locale::PtBr) => "descartar todas as mudanças com `git restore .`",
        ("command_guard.restore_all", Locale::EnUs) => "discarding every change with `git restore .`",
        ("command_guard.delete_base", Locale::PtBr) => "apagar a branch de integração `{branch}`",
        ("command_guard.delete_base", Locale::EnUs) => "deleting the integration branch `{branch}`",
        ("command_guard.windows_path", Locale::PtBr) => {
            "Comando barrado: o destino `{target}` é um caminho do Windows, e o terminal do Bash não \
             entende esse formato (no Windows vira um arquivo de nome estranho na pasta atual; no \
             Linux e no macOS, um arquivo chamado `{target}`).\nNo Windows, use a forma \
             `/c/pasta/arquivo`; no Linux e no macOS, um caminho absoluto de verdade. Caminho \
             relativo funciona em todos.\nComando: {command}"
        }
        ("command_guard.windows_path", Locale::EnUs) => {
            "Command blocked: the target `{target}` is a Windows path, and the Bash terminal does not \
             understand that form (on Windows it becomes an oddly named file in the current folder; \
             on Linux and macOS, a file named `{target}`).\nOn Windows, use the `/c/folder/file` \
             form; on Linux and macOS, a real absolute path. A relative path works everywhere.\n\
             Command: {command}"
        }
        ("base.unmeasured", Locale::PtBr) => {
            "Não dá para saber de qual branch cortar: este projeto não declara base nenhuma em \
             `mustard.json#git.flow`, o remoto não respondeu qual é a branch padrão dele e o \
             checkout não está em branch nenhuma. Diga a base com `--base <branch>` ou declare o \
             `git.flow`. Nada foi cortado."
        }
        ("base.unmeasured", Locale::EnUs) => {
            "There is no branch to cut from: this project declares no base in \
             `mustard.json#git.flow`, the remote did not answer which its default branch is, and \
             the checkout is on no branch. Name the base with `--base <branch>` or declare \
             `git.flow`. Nothing was cut."
        }

        // Defeitos de clareza de um texto (`domain::clarity`) — cada um é uma
        // linha curta que abre com o erro e fecha, depois dos dois-pontos ou
        // do ponto e vírgula, com o jeito de consertar. A recusa de uma lição
        // leva a linha inteira; a mensagem seguinte a uma resposta leva só o
        // erro, o trecho antes da primeira pontuação dessas
        // (`clarity_check::error_of`). Sem parênteses: o tom técnico os
        // apagaria. `{words}`, `{opening}`, `{acronym}`, `{code}`, `{lines}`,
        // `{limit}`, `{score}`, `{min}`, `{found}` e `{expected}` vêm do
        // chamador.
        ("clarity.long_sentence", Locale::PtBr) => {
            "frase com {words} palavras: \"{opening}…\"; diga a mesma ideia em frases curtas"
        }
        ("clarity.long_sentence", Locale::EnUs) => {
            "sentence with {words} words: \"{opening}…\"; say the same idea in short sentences"
        }
        ("clarity.unexpanded_acronym", Locale::PtBr) => {
            "{acronym} é uma sigla sem explicação; diga o nome por extenso"
        }
        ("clarity.unexpanded_acronym", Locale::EnUs) => {
            "{acronym} is an unexplained acronym; spell out its full name"
        }
        ("clarity.internal_code", Locale::PtBr) => {
            "{code} é um código interno; diga o assunto pelo nome"
        }
        ("clarity.internal_code", Locale::EnUs) => "{code} is an internal code; name the subject instead",
        // O texto longo pede um resumo curto, como o da nota de leitura
        // baixa, e manda o JSON, a tabela ou o documento pedido para a página
        // avulsa: o chat fica com o resumo.
        ("clarity.too_long", Locale::PtBr) => {
            "resposta com {lines} linhas, e o limite é {limit}; faça no chat um resumo curto, e \
             JSON, tabela ou documento pedido vai para a página avulsa: `mustard-rt run page`"
        }
        ("clarity.too_long", Locale::EnUs) => {
            "reply with {lines} lines, and the limit is {limit}; write a short summary in the chat, \
             and put a requested JSON, table or document on its own page: `mustard-rt run page`"
        }
        ("clarity.hard_to_read", Locale::PtBr) => {
            "texto difícil de ler: nota {score} no índice de Flesch, e o mínimo é {min}; faça um \
             resumo curto em palavras simples"
        }
        ("clarity.hard_to_read", Locale::EnUs) => {
            "hard to read: {score} on the Flesch reading-ease index, and the minimum is {min}; \
             write a short summary in plain words"
        }
        // A prosa saiu num idioma que não é o do projeto, que é o do usuário.
        // `{found}` e `{expected}` são códigos de idioma: pt-BR, en-US.
        ("clarity.wrong_language", Locale::PtBr) => {
            "resposta em {found}; o idioma do projeto e do usuário é {expected}; faça um resumo \
             em {expected}"
        }
        ("clarity.wrong_language", Locale::EnUs) => {
            "reply in {found}; the language of the project and the user is {expected}; write a \
             summary in {expected}"
        }
        // A frase curta que a linha escondida da mensagem seguinte leva
        // depois de uma resposta com erro de escrita
        // (`apps/rt/src/hooks/task/clarity_check.rs`), para o assistente
        // corrigir na resposta seguinte. A resposta não é barrada, e a frase
        // não aparece na tela. `{errors}` vem do chamador: os erros, separados
        // por ponto e vírgula.
        ("clarity.next.head", Locale::PtBr) => "Na última resposta: {errors}.",
        ("clarity.next.head", Locale::EnUs) => "In the last reply: {errors}.",
        // O último item da lista quando há mais erros do que ela mostra.
        ("clarity.more", Locale::PtBr) => "e mais {count}",
        ("clarity.more", Locale::EnUs) => "and {count} more",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::translate;

    /// Esta parte guarda as mesmas chaves, com os mesmos textos nos dois
    /// idiomas. Quem muda um texto de propósito grava aqui os dois números
    /// novos que a falha mostra.
    #[test]
    fn the_part_keeps_its_keys_and_texts() {
        crate::platform::i18n::tests::assert_part_unchanged(
            include_str!("gates.rs"),
            super::PREFIXES,
            62,
            0x4097_d153_44e2_f38a,
        );
    }

    /// The messages of the write gate and of the approval witness come from
    /// the catalog in both languages, each with the slots the hook fills.
    #[test]
    fn i18n_translates_write_gate_and_witness_keys() {
        for (key, slots) in [
            ("write_gate.secret", &["{file}", "{pattern}"][..]),
            ("write_gate.spec_file", &["{file}", "{spec}"][..]),
            ("write_gate.not_approved", &["{spec}", "{file}"][..]),
            ("write_gate.on_base", &["{branch}"][..]),
            ("write_gate.unreadable_config", &["{file}"][..]),
            ("write_gate.other_branch", &["{spec}", "{branch}", "{current}"][..]),
            ("approval.witness.clear", &["{spec}"][..]),
            ("approval.witness.free_text", &["{spec}", "{selected}", "{offered}"][..]),
            ("approval.witness.not_affirmative", &["{spec}", "{selected}"][..]),
            ("approval.witness.no_plan", &[][..]),
            ("approval.witness.already", &["{spec}"][..]),
            ("approval.witness.unmet", &["{spec}", "{unmet}"][..]),
            ("change.witness.accepted", &["{code}"][..]),
            ("change.witness.declined", &["{code}"][..]),
            ("change.witness.free_text", &["{code}", "{selected}", "{offered}"][..]),
            ("change.witness.no_spec", &["{code}"][..]),
            ("subagent.ticket_unreadable", &["{ticket}", "{found}"][..]),
            ("subagent.not_approved", &["{spec}", "{phase}"][..]),
            ("subagent.no_wave", &["{spec}", "{wave}"][..]),
            ("session.merged", &["{count}", "{branches}"][..]),
            ("session.project_page", &["{template}", "{capabilities}"][..]),
            ("session.landed", &["{pr}", "{spec}"][..]),
            ("session.provider_silent", &["{spec}", "{reason}"][..]),
            ("session.submodules", &["{spec}", "{text}"][..]),
            ("session.version.drift", &["{stamped}", "{running}"][..]),
            ("session.version.stale", &["{running}", "{installed}"][..]),
            ("session.version.behind", &["{running}", "{plugin}"][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            assert!(pt.starts_with("[Mustard] ") && en.starts_with("[Mustard] "), "{key}");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
        // A pergunta de aprovação da spec, um dos dois gestos em que a
        // testemunha age.
        assert_eq!(translate("approval.question", Locale::PtBr), "Aprovar esta spec?");
        assert_eq!(translate("approval.question", Locale::EnUs), "Approve this spec?");
        assert_eq!(translate("approval.option", Locale::PtBr), "Aprovar");
        assert_eq!(translate("approval.option", Locale::EnUs), "Approve");
        // O gesto da mudança que parte de um agente: a pergunta leva o código
        // da mudança, e as duas opções são as do catálogo.
        assert_eq!(translate("change.question", Locale::PtBr), "Aceitar a mudança {code}?");
        assert_eq!(translate("change.question", Locale::EnUs), "Accept the change {code}?");
        assert_eq!(translate("change.accept", Locale::PtBr), "Aceitar");
        assert_eq!(translate("change.decline", Locale::PtBr), "Recusar");
        assert_eq!(translate("change.accept", Locale::EnUs), "Accept");
        assert_eq!(translate("change.decline", Locale::EnUs), "Decline");
        for gone in ["workbranch.dirty.note", "workbranch.reconcile.warn"] {
            for lang in [Locale::PtBr, Locale::EnUs] {
                assert_eq!(translate(gone, lang), "<missing-key>", "{gone} left with the branch hook");
            }
        }
    }

    /// As recusas da trava de comandos saem do catálogo nos dois idiomas, cada
    /// uma com as vagas que o chamador preenche.
    #[test]
    fn i18n_translates_command_guard_keys() {
        for (key, slots) in [
            ("command_guard.deny", &["{reason}", "{command}"][..]),
            ("command_guard.rm_recursive_force", &[][..]),
            ("command_guard.force_push", &[][..]),
            ("command_guard.reset_hard", &[][..]),
            ("command_guard.clean_force", &[][..]),
            ("command_guard.checkout_all", &[][..]),
            ("command_guard.restore_all", &[][..]),
            ("command_guard.delete_base", &["{branch}"][..]),
            ("command_guard.windows_path", &["{target}", "{command}"][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
    }

    /// Os defeitos de clareza e a frase da mensagem seguinte saem do catálogo
    /// nos dois idiomas, cada um com as vagas que o medidor preenche. A frase
    /// abre dizendo que o erro foi na última resposta. O pedido do complemento
    /// saiu com o bloqueio da escrita, e o aviso da volta saiu antes dele.
    #[test]
    fn i18n_translates_clarity_defect_keys() {
        for (key, slots) in [
            ("clarity.long_sentence", &["{words}", "{opening}"][..]),
            ("clarity.unexpanded_acronym", &["{acronym}"][..]),
            ("clarity.internal_code", &["{code}"][..]),
            ("clarity.too_long", &["{lines}", "{limit}"][..]),
            ("clarity.hard_to_read", &["{score}", "{min}"][..]),
            ("clarity.wrong_language", &["{found}", "{expected}"][..]),
            ("clarity.next.head", &["{errors}"][..]),
            ("clarity.more", &["{count}"][..]),
        ] {
            let (pt, en) = (translate(key, Locale::PtBr), translate(key, Locale::EnUs));
            assert_ne!(pt, "<missing-key>", "{key} missing in pt-BR");
            assert_ne!(en, "<missing-key>", "{key} missing in en-US");
            assert_ne!(pt, en, "{key} must differ per locale");
            for slot in slots {
                assert!(pt.contains(slot) && en.contains(slot), "{key} lost {slot}");
            }
        }
        assert_eq!(translate("clarity.next.head", Locale::PtBr), "Na última resposta: {errors}.");
        assert_eq!(translate("clarity.next.head", Locale::EnUs), "In the last reply: {errors}.");
        for lang in [Locale::PtBr, Locale::EnUs] {
            assert_eq!(translate("clarity.block.head", lang), "<missing-key>", "the complement request left");
            assert_eq!(translate("clarity.note.head", lang), "<missing-key>", "the warning after the complement left");
        }
    }
}
