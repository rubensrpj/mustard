---
description: Use when the user runs /mustard:continue or asks to pick a spec back up where it stopped. The reserve button — resuming already happens on its own at the start of a session.
argument-hint: [spec-name]
---
<!-- mustard:generated -->
# /mustard:continue — Pick the spec back up

`/mustard:continue [spec]` resumes a spec where it stopped. Without a name it resumes the spec of the checkout's branch. It is the **reserve button**: the resume already happens on its own at the start of a session, so typing it is never required.

## One call

```bash
rtk mustard-rt run resume --spec {spec}
```

Without a spec name, drop the flag — the command reads the spec of the checkout's branch, then the spec bound to the session, and refuses when there is none.

The command reads the state and nothing else. It answers `phase` (where the spec stands), `next` (the next step in words) and `command` (the command that performs it). Relay `next` to the user in their own language and run `command` when there is one. Never decide the next step yourself: the phase decides it, and the command says so.

## What it never carries

**No page link.** The address of the spec page lives in the status line, never in the conversation. The resume answers no URL, and neither should you.

**No approval.** A spec that comes back in the plan phase still gets the one question, *"Aprovar esta spec?"*, with **Aprovar** and **Ajustar**, after its page is published. Typing this command approves nothing.

## Edge cases

Unknown spec name → the command refuses with `no-spec-file`; say the spec was not found and stop. No current spec → it refuses with `no-current-spec`; ask which spec, or open one.
