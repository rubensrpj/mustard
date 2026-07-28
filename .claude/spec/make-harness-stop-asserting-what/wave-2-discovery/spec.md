---
id: wave.make-harness-stop-asserting-what.2-discovery
---

# wave-2-discovery

## Summary

Active-spec discovery stops reporting absence it did not verify: a spec on an unmerged work branch is listed as in-flight, with the branch that holds it.

## Network

- Parent: [[spec.make-harness-stop-asserting-what]]

## Tasks

- [ ] Extend discovery beyond the current working tree: read the spec directories carried by the project's work branches without checking any of them out, the way `git ls-tree <branch> -- .claude/spec/` already answers this question.
- [ ] Mark each entry with where it lives, so an in-flight spec is never confused with one present in the current tree.
- [ ] Keep the fail-open contract intact: a git failure degrades to the current-tree answer with a stated reason, never to a panic and never to a silent empty list.

## Files

- `apps/rt/src/commands/spec/active_specs.rs`
