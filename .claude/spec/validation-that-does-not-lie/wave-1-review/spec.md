---
id: wave.validation-that-does-not-lie.1-review
---

# wave-1-review

## Summary

The Files check looks at the right project, sees framework route paths, and stops calling prose a missing file

## Network

- Parent: [[spec.validation-that-does-not-lie]]

## Tasks

- [ ] In apps/rt/src/commands/review/analyze_validation.rs, stop re-deriving the project root. Take it as a parameter and thread it through BOTH std::env::current_dir() sites (lines 369 and 416 today) and through ref_resolves, replacing its bare Path::new(r) with root.join(r). Update the three production callers to pass crate::shared::context::project_dir(): analyze_validation's own run, pipeline/plan_materialize.rs, and spec/spec_draft.rs. Take root unconditionally — never an Option with an internal current_dir fallback, which would keep the hidden dependency and make the bug conditional.
- [ ] Widen the token character set in backtick_file_refs so routing punctuation no longer disqualifies a path outright. Route groups written in parentheses and dynamic segments written in brackets are standard in a widely used web framework; brace and asterisk forms appear in workspace globs. Today any of them drops the whole token silently.
- [ ] Tighten what counts as a reference in the same pass, because widening the character set makes the opposite defect more likely. A backtick token that is documentation prose rather than a path must stop earning a missing-file warning. Design the two changes together and state the rule you land on in the module docs.
- [ ] Three top-level integration tests in apps/rt/tests/, named validation_resolves_from_any_working_directory, validation_sees_paths_with_punctuated_segments and validation_does_not_flag_prose_as_a_file. They MUST be top-level #[test] fns, NOT inside a cfg(test) mod: the acceptance criteria run them with -- --exact and libtest matches the FULL test path, so a module-nested test reports 0 passed and passes the gate without running. Each must be two-sided: an existing file resolves AND a genuinely absent one still warns, so no test can pass by the check going silent.
