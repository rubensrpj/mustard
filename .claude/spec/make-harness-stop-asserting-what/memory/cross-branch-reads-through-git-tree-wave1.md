---
name: cross-branch-reads-through-git-tree-wave1
description: Cross-branch reads go through `git ls-tree -r` once per branch plus `git show` per blob, never a checkout — and the pure `_text` twins of the parsers are what let branch blobs and on-disk files share one definition of "active"/"done" instead of growing a second one.
spec: make-harness-stop-asserting-what
wave: 1
role: general-purpose
session: 9726b360-8b0a-4758-b5c1-c6fa2eb099c7
recorded: 2026-07-28T00:28:44.072Z
source: wave-close
---

Cross-branch reads go through `git ls-tree -r` once per branch plus `git show` per blob, never a checkout — and the pure `_text` twins of the parsers are what let branch blobs and on-disk files share one definition of "active"/"done" instead of growing a second one.
