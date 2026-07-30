---
name: close-pipeline-confirmation-gate-must-not-wave1
description: close_pipeline's confirmation gate must NOT block on `Confirmation::NotTaken` — `confirm_in_process` deliberately declines criteria whose command rebuilds the running binary, so a "block on any unproven" rule would make mustard unable to close its own specs.
spec: close-eleven-harness-defects-found
wave: 1
role: general-purpose
session: f7d2e96a-2127-432b-be06-a29217d949f8
recorded: 2026-07-29T19:02:04.926Z
source: wave-close
---

close_pipeline's confirmation gate must NOT block on `Confirmation::NotTaken` — `confirm_in_process` deliberately declines criteria whose command rebuilds the running binary, so a "block on any unproven" rule would make mustard unable to close its own specs.
