<!-- mustard:generated at:2026-05-29T00:00:00Z role:general -->
# Exports — mustard-core public surface

Root re-exports let consumers write `use mustard_core::…` without remembering the owning sub-module. Source of truth: `src/lib.rs`.

## Top-level modules

| Path | Re-export | Ref |
|---|---|---|
| `io` | `pub mod io` | `src/io/mod.rs` |
| `domain` | `pub mod domain` | `src/domain/mod.rs` |
| `view` | `pub mod view` | `src/view/mod.rs` |
| `platform` | `pub mod platform` | `src/platform/mod.rs` |
| `time` | `pub use platform::time` | `src/platform/time.rs` |

## Re-exported names (by area)

| Area | Symbols | Owning module | Ref |
|---|---|---|---|
| Spec ViewModels | `SpecView`, `SpecSummary`, `SpecState`, `SpecFilter`, `Stage`, `Outcome`, `Phase`, `Scope`, `Flags`, `WaveView`, `QualityRollup`, `AcceptanceCriterion`, `AcStatus`, `TimelineNode`, `WorkspaceSummary`, `WorkspaceAlert`, … | `domain::model::view` | `src/lib.rs:51` |
| Spec doc I/O | `parse_state`, `read_state`, `write_state`, `serialize_header`, `rewrite_header`, `header_field`, `stage_label`, `outcome_label`, `status_word` | `domain::spec` | `src/lib.rs:60` |
| Economy | `EconomyScope`, `EconomySummary`, `SavingsSource` | `domain::economy` | `src/lib.rs:66` |
| Project config | `ProjectConfig`, `Commands`, `GitConfig`, `GateModes`, `Runtime`, `Subprojects`, `RolePattern`, `Amend`, `glob_matches`, `BUILD_COMMAND_FALLBACK` | `domain::config` | `src/lib.rs:72` |
| Command detect | `detect_commands` | `domain::command_detect` | `src/lib.rs:78` |
| Meta sidecar | `Meta`, `MetaFlags`, `read_meta`, `write_meta`, `normalise_lang` | `domain::meta` | `src/lib.rs:84` |
| i18n | `I18n`, `SupportedLocale`, `UserLocale`, `Tone`, `translate`, `apply_tone`, `slugify`, `wave_label`, `LocaleError` | `platform::i18n` | `src/lib.rs:101` |
| Paths | `ClaudePaths`, `SpecPaths`, `WavePaths`, `ClaudePathsError` | `io::claude_paths` | `src/lib.rs:109` |
| Workspace root | `workspace_root`, `WorkspaceError` | `io::workspace` | `src/lib.rs:113` |
| Atomic markdown | `MarkdownDoc`, `MarkdownStore` | `io::atomic_md` | `src/lib.rs:117` |
| Summary doc | `SpecSummaryDoc` | `view::summary` | `src/lib.rs:122` |
| Events | `Event`, `EventReader` | `io::events` | `src/lib.rs:127` |
| Vocabulary | `VocabularyMatcher`, `VocabularyDoc`, `VocabLayer`, `Layer`, `ScanHit`, `PromotionVerdict`, `check_layer_promotion`, `VocabError` | `domain::vocabulary` | `src/lib.rs:133` |

## Not re-exported (module-qualified only)

| Concept | Path | Ref |
|---|---|---|
| Filesystem port + free fns | `io::fs::{Fs, read_to_string, write_atomic, append_line, …}` | `src/io/fs/mod.rs` |
| Real / fake fs | `io::fs::real::RealFs`, `io::fs::memory::FakeFs` | `src/io/fs/` |
| Projections | `view::projection::{project_spec_view, project_workspace, project_waves, project_quality, project_timeline}` | `src/view/projection/` |
| Hook contract | `domain::model::contract::{Check, Observer, HookInput, Verdict, Outcome, Ctx, Trigger}` | `src/domain/model/contract.rs` |
| Harness event | `domain::model::event::{HarnessEvent, Actor, ActorKind, SCHEMA_VERSION}` | `src/domain/model/event.rs` |
| Error | `platform::error::{Error, Result, fail_open, fail_open_with}` | `src/platform/error.rs` |
| Entity registry | `domain::entity_registry` | `src/domain/entity_registry.rs` |
| AST | `domain::ast::{GrammarLoader, TreeSitterParser, …}` | `src/domain/ast/` |
