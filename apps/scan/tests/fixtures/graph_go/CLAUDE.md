@.claude/scan-map.md

# Graph_go

> Parent: [../../../../../CLAUDE.md](../../../../../CLAUDE.md) | Orchestrator: [../../../../../.claude/CLAUDE.md](../../../../../.claude/CLAUDE.md)

## Guards

<!-- mustard:guards -->
<!-- facts: kind=go; frameworks=(none) -->
[critical] never import in internal/model/user.go
This directory is a frozen characterization fixture for `apps/scan/tests/graph_resolution.rs`: the non-regression test pins EXACTLY 1 graph edge whose fan-in target is `internal/model/user.go` — any new internal import, file rename, or extra edge breaks that recorded baseline, so update the test's expectations in the same change or don't touch the shape.
The `module example.test/graphdemo` line in `go.mod` and the import path in `internal/server/server.go` are one contract — module-prefixed resolution is the exact behavior under test, so change them only together and verbatim.
`internal/model/user.go` deliberately samples one of each Go definition shape (struct + method, interface, type alias) with zero imports — extend shapes inside it if needed, but keep it import-free so it stays the pure fan-in target.
Never make this fixture buildable or runnable (no `main`, no dependencies, no `go mod tidy`): the scan miner parses it with tree-sitter and never compiles it — minimality is the spec, and any "fix" toward a real app adds noise the tests will count.
<!-- /mustard:guards -->
