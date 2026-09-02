# Material Rules

The conversation's own channel. `orchestrator.md` classifies the request and `dispatch.md` opens and names the unit; this file is what the unit CARRIES out of the conversation. Why the router ships as separate injectables: `refs/mustard/router-rationale.md`.

## Material

**A decision the conversation settles is written down when it is settled**, never reconstructed from memory at draft time. What is not written before a compaction is lost — measured: two units shipped and NEITHER carried material.

```
mustard-rt run material-add --spec {slug} --kind decision   --subject "<what>"  --detail "<why>"
mustard-rt run material-add --spec {slug} --kind definition --subject "<term>"  --detail "<what it means here>"
mustard-rt run material-add --spec {slug} --kind finding    --subject "<claim>" --detail "<file>" [--line N]
```

One call per item, when it is settled. Each lands in the unit's `spec-material.json`, which is the file `spec-draft --material` reads. **They open from ▸6 on:** the base gate's event log creates `.claude/spec/{slug}/`, so a decision settled before the draft still lands. `unknown_spec` means no gate minted that slug — no unit is open.
