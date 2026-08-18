## Verdict — apps/cli, round 3: REJECTED (1 critical)

Every acceptance criterion passes, and one measured defect blocks.

### Criteria — all green, run individually against the spec's exact commands
| AC | result |
|---|---|
| AC-1..AC-4, AC-11 | `cargo test -p mustard-core --test private_install <name>` → `1 passed` each |
| AC-5 / AC-6 / AC-10 | `cargo test -p mustard-rt --test private_scan|private_surface|private_guards <name>` → `1 passed` each |
| AC-7 | `cargo test -p mustard-cli --test private_init ac7_…` → `1 passed` |
| AC-8 | `cargo test -p mustard-core --test private_install_leaves_no_trace ac8_…` → `1 passed` |
| AC-9 | `cargo build --workspace` → 0 errors (1 pre-existing warning, untouched) |

`cargo test --workspace` → no failure anywhere; `cargo clippy --workspace --all-targets` → 0 errors, no new warning in a changed file. Mold `cli-options-pattern` compliant. `apps/cli` Guards clean. Change requests 1–3 and 4(a)/(c)/(d) are all in the code and covered by a criterion.

### CRITICAL — the enumerated cover is incomplete, and the leak is not hypothetical
`packages/core/src/platform/project_seed.rs:135` — `HARNESS_CLAUDE_OUTPUT` was hand-typed. Change request 4(b) said to derive it "from the declarations that already exist rather than retyped"; that declaration is `DOCUMENTED_DIRS` at `packages/core/src/io/claude_paths.rs:156`, whose own doc says "derive their catalog from it instead of hand-maintaining a duplicate". The duplicate is already wrong.

Built a probe repo carrying exactly this wave's `footprint_rules()` plus the seeded `.claude/.gitignore`, and asked git about every file this repository's harness has really produced:

```
$ git check-ignore --no-index --stdin < allpaths.txt   # 29154 real .claude files
total visible: 18   → all .claude/plans/*.md
$ ... over subproject .claude/ trees
apps/rt/.claude/.metrics/qa.jsonl
```

- `.claude/plans/` is filled **because of Mustard's own seed**: `packages/core/templates/settings.json:4` is `"plansDirectory": ".claude/plans"`. Mustard's own clone needed a hand-written `.git/info/exclude:31` for it.
- `.claude/graph/` is the same class — production `run capability` materializes nodes there (`apps/rt/src/commands/capability/mod.rs:292`). Not in the list.
- `<sub>/.claude/.metrics/` is uncovered from the other direction: the seeded `.claude/.gitignore` governs only the **root** `.claude/`, and no `**/.claude/.metrics/` rule was lifted to depth.

This is not the accepted trade. The user decision accepted a *future* producer staying visible; these are declared today. And under this project's `git add -A` law, "visible" means **staged and committed** — 18 files whose names are the operator's own prompt titles, landing in a client's repository. That is the Success Metric ("`git status` EMPTY") failing on first real use.

### MAJOR — the ratchet is one-directional
`project_seed.rs:1595` `no_rule_reaches_a_depth_that_is_not_ours` asks, for each *emitted* rule, "does it match ours / does it spare theirs". It has no direction that asks "is every path Mustard produces matched by some rule", which is exactly why the gap above shipped green. AC-8 cannot see it either — its `write_harness_output` fixture writes only `scan-map.md`, `CLAUDE.local.md` and `spec/`.

### MINOR
`packages/core/src/platform/error.rs` renders "private install cannot hide" while `apps/cli/src/commands/init.rs:~360` bails with "a private install must not write anything it cannot hide" — two wordings for one refusal; only the core one is asserted.
