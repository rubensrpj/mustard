# Mustard principles

## Final work by the rule

All work starts from the recorded rule, finds every place of it through the map and ends in the rule's own test, with agreed numbers, which fails before and passes after.

- For: conductor, wave, review
- Enforcement: the close runs each criterion's proof on the code from before the work, where it must fail, and on the code after, where it must pass running a test.
- Test: `a_proof_green_before_the_change_or_red_after_it_is_refused`

## The program does and the AI decides

The mechanical part, such as counting, joining, checking and recording, is the program's; the AI keeps only what takes judgment.

- For: conductor, wave
- Enforcement: text only

## Use Mustard and never work beside it

Search, read, record and measure through Mustard's commands, with no script, filter or shortcut beside them, and fix its defects in the product.

- For: conductor, wave, review
- Enforcement: a Mustard answer piped into a script is refused.
- Test: `a_runtime_answer_piped_into_a_script_is_refused`

## The spec only through the program

The spec is read and recorded only through Mustard's commands, and no `spec.*` file is edited by hand.

- For: conductor, wave, review
- Enforcement: every write tool on a spec file is blocked.
- Test: `every_write_tool_on_a_spec_file_is_blocked_with_its_own_message`

## An error found in the work is fixed in it

A defect the work finds on its way is fixed inside it, not in some later work.

- For: conductor, wave
- Enforcement: text only

## A direct rule instead of guessing

What Mustard records is decided by a direct rule that closes every case; a guess is only allowed in free text.

- For: conductor, wave, review
- Enforcement: text only

## A project rule becomes a test

A project rule that can be checked becomes a test in the code, not a lesson or a loose warning.

- For: conductor, wave, review
- Enforcement: a lesson that is a project rule is refused and becomes a test.
- Test: `project_rule_lesson_is_refused_and_becomes_a_test`

## What value is

Value is speed, savings, effectiveness and quality, through orchestration, agents and skills, Haiku and Jev; the rest goes.

- For: conductor, wave, review
- Enforcement: text only

## A safe stop only for what depends on it

A risk stops only the work that depends on it, and the rest goes on.

- For: conductor, wave
- Enforcement: text only

## What lost its purpose goes

The code, test and text the change made useless go in the same wave.

- For: conductor, wave, review
- Enforcement: the close sends back the wave that left unused code in its file.
- Test: `the_unused_analysis_reopens_only_the_wave_whose_file_it_cites`

## Whoever did it does not check it

Whoever did the work does not approve it; someone else checks it before the commit.

- For: conductor, review
- Enforcement: the round commits no unreviewed code, and a change after the review asks for another.
- Test: `usage_cannot_commit_unreviewed_code_and_changed_code_invalidates_the_view`

## Measure with the program and its own version's data

Every measure comes from the program, with the data of the running version, and one sample never becomes a general rule.

- For: conductor, wave, review
- Enforcement: text only

## A real example before changing the product

Before changing the product, show the real case where it failed, seen by the user or recorded.

- For: conductor
- Enforcement: text only

## Agnostic

The product serves any project, language and stack, and nothing in it assumes the project it was born in.

- For: conductor, wave, review
- Enforcement: no wave or review request names a stack's build folder.
- Test: `no_request_names_a_build_folder_in_any_project`

## Nothing of Mustard in git

Mustard's files stay out of the project's git, and no commit carries any of them.

- For: conductor, wave, review
- Enforcement: the install puts Mustard's files on git's local ignore list.
- Test: `private_upsert_writes_clone_local_exclude`

## The project rules over the rules

A project rule wins over Mustard's when both deal with the same point.

- For: conductor, wave, review
- Enforcement: the requests say a project rule on language wins over the languages line.
- Test: `the_wave_and_final_review_requests_say_a_project_rule_on_language_wins_over_the_languages_line`

## Explain from the start without internal codes

Answer in the order of explaining from the response style, in words, without Mustard's internal codes.

- For: conductor
- Enforcement: a catalog text with an internal code or a hard sentence is refused.
- Test: `every_catalog_text_reads_clearly`

## Product decisions belong to the user

A change the user did not authorize waits for their acceptance, and the agent returns the doubt instead of deciding.

- For: conductor, wave, review
- Enforcement: only the user's "Approve" on the approval question approves the spec.
- Test: `a_negation_or_another_questions_option_never_approves`
