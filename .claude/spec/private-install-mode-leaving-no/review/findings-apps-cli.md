## Verdict — apps/cli, round 2

**REJECTED — 1 blocking defect.** The declared criteria pass; a defect they cannot see ships a silent-data-loss trap into the client repo.

### Claims verified (all green)
| Claim | Command | Real output |
|---|---|---|
| AC-7 | `cargo test -p mustard-cli --test private_init ac7_init_private_seeds_no_github_template` | `1 passed (1 suite, 1.78s)` |
| No CLI regression | `cargo test -p mustard-cli` | `50 passed (5 suites, 4.08s)` |
| AC-9 | `cargo build --workspace` | `0 errors, 1 warnings` (pre-existing, `apps/rt/src/commands/feature.rs:488`, untouched) |
| Lints | `cargo clippy -p mustard-cli --all-targets` | `0 errors, 30 warnings` — all pre-existing files |

Independent field proof (not the test suite): drove the real `target/debug/mustard.exe init --yes --private` in a fresh git host repo that already versioned its own `CLAUDE.md`, then planted a subproject `.claude/`, a `CLAUDE.local.md`, a `.claude.backup.20260817T120000/` and a `.claude/spec/foo/` — `git status --porcelain --untracked-files=all` came back **empty**, `CLAUDE.md` untouched. Re-running `init --yes` with **no flag** stayed private (`kept .claude/settings.local.json`, no `settings.json`, no `.github/`), so the autodetection seam really works through the CLI. Switching an already-shared install to `--private` printed the correct residue report and the `git rm --cached` line. Guards checked: no `unwrap`/`expect` outside `#[cfg(test)]` in the new code, `mustard.json` stays root-anchored (`/mustard.json`), `.claude` skip-list and the fail-open probes untouched. Mold `cli-options-pattern` conformant.

### CRITICAL — a private install hides a file it never writes
`apps/cli/src/commands/init.rs:262` refuses to seed `.github/` precisely because "it lands outside `.claude/`, where nothing else covers it". Then `hide_footprint` at `apps/cli/src/commands/init.rs:356` writes **every** `footprint_rules()` entry into the clone-local exclude file — including `.github/pull_request_template.md` (`packages/core/src/platform/project_seed.rs:244`, `seeded(GITHUB_PR_TEMPLATE)`). Measured in the fixture above:

```
$ git check-ignore -v .github/pull_request_template.md
.git/info/exclude:15:.github/pull_request_template.md   .github/pull_request_template.md
$ git status --porcelain --untracked-files=all
?? .github/workflows/ci.yml          <- the client's other file shows; their PR template does not
```

So in the mode's own headline scenario — a fresh `--private` install into a client repo — a PR template the client (or the operator, for the client) authors is invisible to `git status` and skipped by `git add -A`, silently, forever. This is the *identical* failure the wave already reasoned about and closed for `CLAUDE.md`: `FootprintEntry` doc at `packages/core/src/platform/project_seed.rs:144` ("a rule would hide an instruction file the operator authored FOR the client from that client's own `git add -A`") and the property test `no_rule_reaches_a_depth_that_is_not_ours` at `:1499` ("a private install never writes a `CLAUDE.md`, so it must never hide one"). The PR template is in exactly that class under this mode, yet it is `seeded(..)` instead of the `watched(..)`-shaped entry (rule `None`, pathspec kept so the residue report still names it, as it correctly did in the shared→private switch run). The property test passes only because the rule carries an interior slash — it checks anchoring, not "is this ours".

### Non-blocking
- `apps/cli/src/commands/init.rs:213` — the comment claims step 0 hides the footprint "BEFORE any of it is written", but the interactive backup-and-overwrite branch creates `.claude.backup.<stamp>/` at `:198`, before `hide_footprint` at `:216`. End state is still clean (an exclude rule applies to an untracked path whenever it is added), so this is a doc inaccuracy, not a leak.
- `apps/cli/tests/private_init.rs:194` — the "the rules cover OUR footprint and stop there" loop tries only `CLAUDE.md` and `services/billing/mustard.json`; adding `.github/pull_request_template.md` to that list is what would have caught the critical above. The CLI-side autodetection re-run also has no regression guard.
