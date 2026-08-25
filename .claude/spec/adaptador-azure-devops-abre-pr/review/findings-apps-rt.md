## Verdict — approved (critical: 0)

T1-T5 all PASS: injectable transport with real ureq + recorded fake; pure pat_from precedence (env → git vault → refusal naming both); URLs derived from all remote spellings, response fields ignored (decoy-tested); four operations normalized; no merge anywhere (transport rejects verbs outside GET/POST/PATCH). No unwrap/expect outside tests. shared:: 80 passed; full lib 2010 passed (1 pre-existing writer_ndjson latency flake, untouched, passes isolated).

Minor (non-blocking): do_view_branch (pr_azure.rs:406) does not percent-encode the branch in searchCriteria; '+' or '%' in a branch name would misdecode into a false no-pr-for-branch.
