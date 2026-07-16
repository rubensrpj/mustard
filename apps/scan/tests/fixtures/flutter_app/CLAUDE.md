# Flutter_app

> Parent: [../../../../../CLAUDE.md](../../../../../CLAUDE.md) | Orchestrator: [../../../../../.claude/CLAUDE.md](../../../../../.claude/CLAUDE.md)

## Guards

<!-- mustard:guards -->
<!-- facts: kind=pub; frameworks=sdk, flutter, collection, flutter_test, flutter_lints -->
[critical] never `part 'counter.g.dart'` in `apps/scan/tests/fixtures/flutter_app/**`
This directory is a frozen scan fixture, not an app: it is never built or run — don't run `flutter pub get`/`build_runner` here or commit `pubspec.lock`/`.dart_tool/`; any new file silently enlarges the mined surface that `apps/scan/tests/dart_mining_e2e.rs` asserts.
`lib/counter.g.dart` is deliberately orphaned (its `part of 'counter.dart'` has no counterpart on purpose): it exists solely so the `**/*.g.dart` path marker classes it `generated` — never "fix" the dangling part, add a `counter.dart`, or hand-edit the file.
The identifiers in `lib/main.dart` are asserted by name in `dart_mining_e2e.rs` (`MyApp`, `CounterPage`, the `StatelessWidget` supertype, the `package:flutter/material.dart` import) — rename or drop any of them only in lockstep with that test.
`pubspec.yaml` is stack-detection evidence, not a dependency list: the e2e requires exactly ONE detected stack (`flutter`, with dep + path + code signals converging) — a dependency from another stack's registry breaks the `stacks.len() == 1` assertion.
If scan output over this fixture looks wrong, fix the data registries (`languages.toml`, `queries/dart/*.scm`, `generated-markers.toml`) — never reshape the fixture to make a test pass.
<!-- /mustard:guards -->

<!-- mustard:scan-map -->
Tipo: pub · 2 arquivos
O terreno já está na sua janela (o census de orientação injetado no início da sessão). Para localizar: `grep` para termo exato conhecido; `mustard-rt run feature` (digest) para conceito; depois leia os arquivos apontados — o digest acha onde olhar, não substitui ler.
<!-- /mustard:scan-map -->
