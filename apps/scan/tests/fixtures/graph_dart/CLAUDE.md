@.claude/scan-map.md

# Graph_dart

> Parent: [../../../../../CLAUDE.md](../../../../../CLAUDE.md) | Orchestrator: [../../../../../.claude/CLAUDE.md](../../../../../.claude/CLAUDE.md)

## Guards

<!-- mustard:guards -->
<!-- facts: kind=pub; frameworks=(none) -->
This fixture is one leg of a two-way contract with `queries/kinds-manifest.toml` `[dart]`: every declared kind (class, mixin, enum, extension, method) must yield at least one declaration in `lib/models.dart`, and the file must yield no undeclared kind — `tests/kinds_parity.rs` fails in either direction.
Never remove or rename a construct (e.g. `mixin Auditable`) on its own — that trips the "declared kind produced no declaration" check; change it only together with the matching edit to `queries/kinds-manifest.toml` and `queries/dart/tags.scm`.
Before adding a construct, check what the Dart `tags.scm` captures: `method` comes from `function_signature`, so keep methods as explicit signatures — the file deliberately avoids getters, fields-as-declarations, and anything else that would surface an undeclared kind.
This is scan input, never compiled or run: keep `pubspec.yaml` a bare `name` + `environment` stub (it exists only so the scanner classes the root as a pub package) — do not add `dependencies` or try to make the Dart "real".
The header comment in `lib/models.dart` is the fixture's spec — if you change what the file exercises, update that comment in the same edit.
<!-- /mustard:guards -->
