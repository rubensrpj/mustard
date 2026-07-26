---
name: negative-check-deliberately-does-not-set-wave2
description: ac-negative-check deliberately does NOT set QaRunOptions::self_invoked — at plan time the AC's named test does not exist, so `cargo test <name>` exits 0 with "0 passed" and the Expect regex is what turns it red; setting the flag would skip every cargo criterion and report them all unproven.
spec: prove-every-acceptance-criterion-can
wave: 2
role: general-purpose
session: eb8504c5-f25b-4d3e-874e-d99047db16a5
recorded: 2026-07-26T02:44:15.930Z
source: wave-close
---

ac-negative-check deliberately does NOT set QaRunOptions::self_invoked — at plan time the AC's named test does not exist, so `cargo test <name>` exits 0 with "0 passed" and the Expect regex is what turns it red; setting the flag would skip every cargo criterion and report them all unproven.
