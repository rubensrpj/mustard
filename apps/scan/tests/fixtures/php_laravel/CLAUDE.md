# Php_laravel

> Parent: [../../../../../CLAUDE.md](../../../../../CLAUDE.md) | Orchestrator: [../../../../../.claude/CLAUDE.md](../../../../../.claude/CLAUDE.md)

## Guards

<!-- mustard:guards -->
<!-- facts: kind=composer; frameworks=php, laravel/framework, guzzlehttp/guzzle, phpunit/phpunit, mockery/mockery; scripts=post-autoload-dump: @php artisan package:discover --ansi, test: phpunit -->
This directory is a committed test fixture, not a runnable app: never run `composer install` here nor add `vendor/` or `composer.lock` — `php_laravel_fixture.rs` and friends scan the tree exactly as committed.
Dependency ORDER in `composer.json` is asserted verbatim (`require` before `require-dev`, document order within each) to prove serde_json `preserve_order` end-to-end — never reorder, add, or drop a dep without updating `php_laravel_fixture.rs::composer_manifest_carries_deps_scripts_in_document_order`.
`stack_detection_e2e.rs` asserts EXACTLY one detected stack with the exact signal triple `dep:laravel/framework` + `path:routes/web.php` + `code:Illuminate\Support\Facades` — keep `routes/web.php` at that path with its `Illuminate\Support\Facades\Route` import, and don't introduce a dep that could fire a second detection.
`composer.json` must stay at the fixture root: the per-unit detection test locates the root unit by `dir == ""`, and `anchor_ranking.rs` expects the query `user` to hit — keep a `User`-named symbol under `app/Models/`.
Any shape change here ripples across five test files (`php_laravel_fixture`, `stack_detection_e2e`, `anchor_ranking`, `stack_evidence_excludes` copies this tree wholesale, `graph_resolution` mirrors it) — run `cargo test -p scan` after touching anything.
<!-- /mustard:guards -->

<!-- mustard:scan-map -->
Tipo: composer · 2 arquivos
O terreno já está na sua janela (o census de orientação injetado no início da sessão). Para localizar: `grep` para termo exato conhecido; `mustard-rt run feature` (digest) para conceito; depois leia os arquivos apontados — o digest acha onde olhar, não substitui ler.
<!-- /mustard:scan-map -->
