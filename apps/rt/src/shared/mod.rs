//! `shared` — cross-face infrastructure consumed by **both** the enforcement
//! face (`hooks`) and the script face (`commands`).
//!
//! Keeping these here (instead of under `commands/`) preserves a clean
//! dependency DAG: `hooks` and `commands` both depend on `shared`, and `shared`
//! never depends back on either. A hook reaching into a command module would
//! invert that layering — this module exists to make that impossible.
//!
//! - [`branch_state`] — the ONE sweep of work-unit branches (local AND remote)
//!   and the classifier that says what state each is in, reading git directly
//!   and asking about pull requests only when the consumer says to
//!   ([`branch_state::PrQuery`]). Both faces ask it: the exit ritual
//!   (`commands::git_settle`), the spec inventory and the statusline.
//! - [`context`] — run-context resolution (cwd / session-id / current-spec),
//!   the port of `hook-env.js`'s runtime probing.
//! - [`events`] — the NDJSON event bus: classification/routing ([`events::route`])
//!   and the append-only writer ([`events::writer_ndjson`]).
//! - [`prompt`] — tells a person's prompt apart from the runtime's own notices,
//!   which reach the session through the same `UserPromptSubmit` channel. One
//!   owner for the rule, shared by every observer on that trigger.
//! - [`spec_state`] — the ONE ladder that names the current spec (the
//!   environment override, then the checkout's branch, then the session
//!   binding). Every door that asks "which spec is this" goes through it.
//! - [`pr_provider`] — the pull-request ACTIONS (open/edit/ready/view) as a
//!   port: callers depend on the trait, adapters are the only place a provider
//!   and its CLI/API are named, and the factory picks by the provider in force.
//!   A leitura do estado, ao lado, não é porta — ver `branch_state` acima.
//! - [`pr_azure`] — the Azure DevOps adapter behind that port: the Git REST
//!   API over an injectable transport, the PAT from `AZURE_DEVOPS_EXT_PAT` or
//!   the git credential vault, every URL derived from the `origin` remote —
//!   and deliberately no merge operation.
//! - [`jev`] — the map-search filter over the Jev paid service, behind the
//!   core's `MapFilter` port: candidates in parallel groups, the core's cut,
//!   and the machine-wide key that never leaves the `Authorization` header.
//! - [`proc`] — signal-free, cross-platform process primitives (the liveness
//!   probe) plus [`proc::run_shell_with_deadline`]
//!   — the ONE shell-command runner that drains both pipes concurrently and
//!   waits under a deadline, shared by `verify-pipeline` and `qa-run`.
//! - [`work_kind`] — WHAT a work unit is (`feature`/`fix`/`hotfix`), the
//!   `{kind}/{slug}` name built from it, and the project's base model derived
//!   from `git.flow`. The crate's ONE parser of a work-branch name, in both the
//!   current shape and the `{base}_{slug}` shape units in flight still carry —
//!   and the one reader/writer of the base a unit was actually CUT from, which
//!   only the unit's own record can remember once the pending marker is
//!   consumed.

pub mod branch_state;
/// O caminho do código pelo mapa, para as travas da leitura e da busca.
pub mod code_route;
/// A chave do Jev no arquivo de configuração, que nenhuma leitura mostra.
pub mod config_key;
pub mod context;
/// One topological level assignment for the whole crate — see the module docs
/// for why there used to be two, and what they disagreed about. Also the
/// backlog: the same peel over a task graph instead of a wave graph, plus
/// readiness, packing into dispatch batches under a work cap (tasks and
/// files), and the waiting task that joins the batch it depends on.
// A rodada chama o backlog. O allow segura só `task_levels`, que nenhum
// código de produção chama: só os testes dele.
#[allow(dead_code)]
pub mod dag;
pub mod jev;
pub mod paths;
// The Azure adapter behind the pr_provider port — reached through the factory.
pub mod pr_azure;
pub mod pr_provider;
pub mod pr_history;
pub mod proc;
pub mod prompt;
/// O texto do catálogo com as vagas preenchidas.
pub mod say;
pub mod spec_state;
// Test-only: cloning git fixture scenery instead of rebuilding it per test.
#[cfg(test)]
pub mod test_fixture;
/// A resposta do mapa no lugar da busca por palavra do Claude.
pub mod word_search;
pub mod work_kind;

// Veio da economia quando ela saiu: a barra de status le o ganho do rtk.
pub mod rtk_gain;
