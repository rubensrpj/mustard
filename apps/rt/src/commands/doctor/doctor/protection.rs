//! A proteção das bases: o que o provedor do projeto realmente recusa em cada
//! base que o `git.flow` declara, e o aviso quando o projeto não declara base
//! nenhuma.

use std::path::Path;

use mustard_core::platform::i18n::{translate, Locale};

use super::CheckResult;

/// Ask the PROVIDER whether each base this project declares is protected, and
/// warn when the project declares none.
///
/// **Why the provider and not this machine.** Everything the harness knows
/// about a base is true only on this side of the wire: the write gate refuses
/// an edit here, the doors refuse a merge here. A colleague with a terminal and
/// push rights is stopped by the server's own rule or by nothing at all — so
/// the one reading worth reporting is the server's, and it is asked for every
/// base the project named.
///
/// **Why an absent `git.flow` is a finding again.** The bases come from the
/// declaration alone; with none declared, nothing is protected, nothing is
/// pre-selected, and no base can be asked about. That used to be reported as a
/// healthy install because protection then rested on a probe of `origin/HEAD` —
/// it no longer does, and staying silent would leave the operator believing in
/// a protection that does not exist.
///
/// Skipped when there is no `mustard.json` at the project root (not a mustard
/// project). Never fails: a provider that cannot be reached is reported as
/// unasked, which is a different sentence from unprotected.
pub(super) fn check_branch_protection(cwd: &Path, lang: Locale) -> CheckResult {
    const NAME: &str = "branch-protection";
    if !cwd.join("mustard.json").is_file() {
        return CheckResult::skip(NAME, "no mustard.json at project root");
    }
    let config = mustard_core::ProjectConfig::load(cwd);
    let declared: Vec<String> = config.git.declared_bases().into_iter().collect();
    if declared.is_empty() {
        return CheckResult::warn(
            NAME,
            vec![
                translate("doctor.protection.flow_missing", lang).to_string(),
                translate("doctor.protection.fix", lang).to_string(),
            ],
        );
    }

    protection_report(&declared, crate::shared::pr_provider::provider_for(cwd).as_ref(), lang)
}

/// The reading itself, over the bases and whoever answers for them.
///
/// Apart from [`check_branch_protection`] so the report can be measured
/// against a provider that answers a KNOWN mix — one base ruled, one base
/// open — which is the case the whole check exists for and the one no
/// temporary directory can produce.
fn protection_report(
    declared: &[String],
    provider: &dyn crate::shared::pr_provider::PrProvider,
    lang: Locale,
) -> CheckResult {
    const NAME: &str = "branch-protection";
    let who = provider.provider().to_string();
    let mut details = Vec::new();
    let mut open_bases = false;
    for base in declared {
        let line: String = match provider.branch_protection(base) {
            Ok(true) => translate("doctor.protection.protected", lang).to_string(),
            Ok(false) => {
                open_bases = true;
                translate("doctor.protection.open", lang).to_string()
            }
            Err(reason) => {
                translate("doctor.protection.unasked", lang).replace("{reason}", reason.trim())
            }
        };
        details.push(line.replace("{base}", base).replace("{provider}", &who));
    }
    if open_bases {
        details.push(translate("doctor.protection.fix", lang).to_string());
        return CheckResult::warn(NAME, details);
    }
    let mut r = CheckResult::ok(NAME);
    r.details = details;
    r
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::super::Status;
    use super::*;
    use crate::commands::doctor::doctor::tests::*;

    /// Sem `git.flow`, nada fica protegido — e isso é um achado, não uma
    /// instalação saudável. O aviso diz o `git.flow` pelo nome e mostra como
    /// declarar.
    #[test]
    fn a_falta_do_fluxo_vira_aviso_porque_nada_fica_protegido() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("mustard.json"),
            r#"{"git":{"flow":{},"provider":"github"}}"#,
        );
        let result = check_branch_protection(dir.path(), Locale::PtBr);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        assert!(
            result.details.iter().any(|d| d.contains("git.flow")),
            "o aviso precisa dizer o que falta: {:?}",
            result.details
        );
        assert!(
            result.details.iter().any(|d| d.contains("mustard init")),
            "e como declarar: {:?}",
            result.details
        );
    }

    /// Com bases declaradas e um provedor que não responde, cada base é
    /// reportada como PERGUNTA QUE NÃO CHEGOU A SER FEITA — nunca como
    /// desprotegida, que é uma afirmação sobre o servidor.
    #[test]
    fn provedor_fora_de_alcance_nao_vira_base_desprotegida() {
        let dir = tempdir().unwrap();
        write_file(
            &dir.path().join("mustard.json"),
            r#"{"git":{"flow":{"*":"develop","develop":"master"},"provider":"naoexiste"}}"#,
        );
        let result = check_branch_protection(dir.path(), Locale::PtBr);
        for base in ["develop", "master"] {
            assert!(
                result.details.iter().any(|d| d.contains(base)),
                "cada base declarada é reportada, {base} não foi: {:?}",
                result.details
            );
        }
        assert!(
            result.details.iter().all(|d| !d.contains("qualquer pessoa")),
            "um provedor que não respondeu não prova branch aberta: {:?}",
            result.details
        );
    }

    /// Um projeto que declara `develop` e `master` e um provedor que, para a
    /// `master`, não tem política nenhuma: o diagnóstico acusa a `master`,
    /// deixa a `develop` em paz e mostra como ligar.
    ///
    /// Este é o caso que a conferência existe para pegar, e nenhuma pasta
    /// temporária o produz — por isso o provedor aqui é um dublê que responde
    /// uma mistura conhecida.
    #[test]
    fn o_diagnostico_acusa_a_base_que_o_provedor_nao_protege() {
        /// Responde `true` para as bases que nomeia e `false` para as outras.
        struct ProvedorFalso(&'static [&'static str]);
        impl crate::shared::pr_provider::PrProvider for ProvedorFalso {
            fn provider(&self) -> &'static str {
                "azure"
            }
            fn open(
                &self,
                _pr: &crate::shared::pr_provider::PrToOpen,
            ) -> Result<crate::shared::pr_provider::PrOpened, String> {
                Err("fora do teste".into())
            }
            fn edit_body(&self, _n: u64, _b: &str) -> Result<(), String> {
                Err("fora do teste".into())
            }
            fn ready(&self, _n: u64) -> Result<(), String> {
                Err("fora do teste".into())
            }
            fn view(
                &self,
                _which: crate::shared::pr_provider::PrRef<'_>,
            ) -> Result<crate::shared::pr_provider::PrView, String> {
                Err("fora do teste".into())
            }
            fn checks(
                &self,
                _n: u64,
            ) -> Result<crate::shared::pr_provider::PrChecks, String> {
                Err("fora do teste".into())
            }
            fn branch_protection(&self, branch: &str) -> Result<bool, String> {
                Ok(self.0.contains(&branch))
            }
        }

        let bases = vec!["develop".to_string(), "master".to_string()];
        let result =
            protection_report(&bases, &ProvedorFalso(&["develop"]), Locale::PtBr);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        let acusa = result
            .details
            .iter()
            .find(|d| d.contains("master"))
            .unwrap_or_else(|| panic!("a master não foi acusada: {:?}", result.details));
        assert!(
            acusa.contains("qualquer pessoa"),
            "o aviso precisa dizer o que uma base sem regra significa: {acusa}",
        );
        assert!(
            result.details.iter().any(|d| d.contains("Rulesets") || d.contains("Policies")),
            "e mostrar como ligar: {:?}",
            result.details
        );
        let develop = result
            .details
            .iter()
            .find(|d| d.contains("develop"))
            .unwrap_or_else(|| panic!("a develop sumiu do relatório: {:?}", result.details));
        assert!(
            !develop.contains("qualquer pessoa"),
            "a base que o provedor protege não pode ser acusada: {develop}",
        );

        let tudo_protegido =
            protection_report(&bases, &ProvedorFalso(&["develop", "master"]), Locale::PtBr);
        assert_eq!(
            tudo_protegido.status,
            Status::Ok,
            "com as duas protegidas não há achado: {:?}",
            tudo_protegido.details
        );
    }

    #[test]
    fn branch_protection_missing_mustard_json_skips() {
        let dir = tempdir().unwrap();
        let result = check_branch_protection(dir.path(), Locale::PtBr);
        assert_eq!(result.status, Status::Skip, "{:?}", result.details);
    }
}
