---
qualifyingSignals: [role:ui, role:frontend, role:mobile-web, stack:react, stack:vue, stack:svelte, stack:next]
---
# Browser Debug Playbook (Frontend Bugfix)

> Detail for `/bugfix` when role=ui. Loaded on demand.
> **Purpose:** systematic FE debugging using Playwright MCP + Chrome DevTools MCP — the canonical 2026 pair (cf. [Steve Kinney — Playwright vs Chrome DevTools MCP](https://stevekinney.com/writing/driving-vs-debugging-the-browser)).

---

## Tool selection

| Tool | Use for | Key MCP commands |
|---|---|---|
| **Playwright MCP** | scripted interaction, repeatable flows, accessibility tree navigation, form automation | `browser_navigate`, `browser_click`, `browser_fill_form`, `browser_snapshot`, `browser_evaluate`, `browser_press_key` |
| **Chrome DevTools MCP** | debugging, performance audits, network inspection, console errors, JS errors | network panel, performance traces, JS exceptions, runtime evaluation |

**Rule of thumb:** Playwright **drives** the browser, DevTools **inspects** it. Use both — they're complementary, not alternatives.

---

## The 5-step loop: reproduce → isolate → instrument → fix → prevent

### Step 1: Reproduce

**Goal:** make the bug fail consistently in a controlled environment before touching code.

1. **Read the report**: extract exact steps, browser, viewport, route, account/role.
2. **Local first**: try to reproduce in dev server before reaching for the browser MCP.
3. **If unstable locally → use Playwright MCP**:
   ```
   browser_navigate → exact failing route
   browser_snapshot → capture initial state
   browser_click / browser_fill_form → drive to the failing step
   browser_snapshot → capture failing state
   ```
4. **Failed to reproduce → STOP and report**: ask user for missing context (auth state, data fixtures, browser version). Do NOT fix what you cannot reproduce.

### Step 2: Isolate

**Goal:** narrow the failure surface to the smallest reproducible unit.

- **Strip the page**: open just the failing component in isolation (storybook entry, sandbox route).
- **Toggle hypotheses**: turn off animations, race conditions, async dependencies one at a time.
- **`browser_console_messages`**: collect all console errors/warnings during repro.
- **`browser_network_requests`**: inspect failing/pending requests — status, timing, payload.
- **Bisect**: if a recent commit broke it, `git bisect` against the failing flow.

### Step 3: Instrument

**Goal:** observe behavior at the exact failure point.

- **`browser_evaluate`**: run targeted expressions to inspect state at the failing step:
  ```js
  browser_evaluate("document.querySelector('[data-testid=...]').dataset")
  browser_evaluate("Array.from(document.querySelectorAll('button')).map(b => b.disabled)")
  ```
- **DevTools network panel**: confirm request payloads and response shapes match expectations.
- **DevTools performance trace**: only if perf-related (TTI, layout shift, jank).
- **Add minimal logs**: `console.log` at hypothesis points — remove before commit.
- **Snapshot accessibility tree**: `browser_snapshot` shows the actual a11y tree the LLM sees — confirms what the user/screen-reader perceives.

### Step 4: Fix

**Goal:** smallest change that resolves root cause, not symptom.

- **Diagnose first**: classify per `pipeline-config.md §Diagnostic Failure Routing`:
  - **Transient**: cache stale, race condition → fix synchronization, not the symptom
  - **Resolvable** (≤3-line patch): null deref, wrong selector → apply patch
  - **Structural**: wrong component layer, missing state machine → re-spec, do not patch
- **Avoid `setTimeout` band-aids**: if you find yourself adding `setTimeout(fn, 100)`, you're masking a race — find the real signal.
- **One change at a time**: don't bundle the fix with refactoring — easier to bisect if it regresses.

### Step 5: Prevent

**Goal:** ensure the bug doesn't return.

- **Add a test**: at the layer where the fix lives (unit, integration, or e2e via Playwright):
  ```
  browser_navigate → failing route
  browser_click → trigger the path
  expect snapshot to match success state
  ```
- **Test the absent-value path**: most FE bugs are unhandled null/empty/undefined — add a test for that exact input.
- **Keyboard test if interactive**: ensure fix works for keyboard users too.
- **Update spec `## Concerns`** if root cause exposes a structural issue worth a follow-up.
- **Add to knowledge base** via `/mustard:knowledge add` if pattern likely to recur — recorded via `mustard-rt run emit-event --event lesson`.

---

## Common FE bug categories (quick map)

| Symptom | Likely category | First MCP call |
|---|---|---|
| "Button does nothing" | event handler not attached / wrong selector | `browser_evaluate` to inspect DOM |
| "Spinner forever" | promise never resolves / unhandled rejection | `browser_console_messages` + `browser_network_requests` |
| "Wrong data shown" | state staleness / cache miss | `browser_evaluate` on store/state |
| "Layout breaks at xs" | responsive regression | `browser_resize` + `browser_snapshot` |
| "Slow load" | bundle bloat / serial requests | DevTools performance + network waterfall |
| "Console error after click" | unhandled exception in async flow | `browser_console_messages` + DevTools sources |
| "Form rejects valid input" | validation schema drift vs server | `browser_evaluate` on form state + check schema file |
| "Modal closes prematurely" | event bubbling / outside click handler too eager | inspect listeners via DevTools |

---

## Anti-patterns (don't do)

- **Screenshot-first**: don't reach for `browser_take_screenshot` before `browser_snapshot`. The accessibility tree is more reliable than image diffs.
- **Reload to fix**: if "F5 makes it work", you're hiding a state initialization bug. Find it.
- **Try-catch swallow**: never `try { ... } catch {}` to make a bug "go away" — surface the error or fix the cause.
- **`setTimeout` to "wait for" something**: use proper async/effect dependencies or `browser_wait_for`.
- **Bug fix + refactor in same diff**: split — review reviewers thank you.

---

## Sources

- [Steve Kinney — Playwright vs Chrome DevTools MCP: Driving vs Debugging](https://stevekinney.com/writing/driving-vs-debugging-the-browser)
- [Webfuse — 5 Best MCP Servers for Browser Automation](https://www.webfuse.com/blog/the-top-5-best-mcp-servers-for-ai-agent-browser-automation)
- [ChromeDevTools/chrome-devtools-mcp](https://github.com/ChromeDevTools/chrome-devtools-mcp)
