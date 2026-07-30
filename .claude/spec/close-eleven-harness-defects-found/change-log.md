# Change Log — close-eleven-harness-defects-found

_Solicitações registradas automaticamente durante o pipeline (mid-spec). O `spec.md` (narrativa congelada) NÃO é alterado; dobre o que muda comportamento em `## Acceptance Criteria` e rode o QA de novo._

- **2026-07-29T19:16:49.017Z** _(Execute)_ — o que você me sugere?
- **2026-07-30T07:26:10.593Z** _(Execute)_ — sim, executa
- **2026-07-30T07:56:58.646Z** _(Execute)_ — segue com o review/qa da onda 1
- **2026-07-30T08:13:18.662Z** _(Execute)_ — continue
- **2026-07-30T08:13:44.294Z** _(Execute)_ — **Instruction:** Name every test you write so its name CONTAINS the exact token the Acceptance Criterion's Command filters for (cargo test filters by substring): AC-7 files_section_reads_a_table_and_names_an_unreadable_one; AC-8 exemplar_files_exclude_machine_written_modules; AC-9 wave_dependency_honours_the_declared_edges; AC-10 emit_phase_confirms_the_transition; AC-11 session_binding_reaches_the_reading_session; AC-12 boundary_warning_names_the_boundary_it_checked; AC-13 work_branch_record_reconciles_with_the_real_branch. Wave 1 named its tests freely and every AC filter matched zero tests - QA would have failed the wave with the behaviour correctly implemented.
