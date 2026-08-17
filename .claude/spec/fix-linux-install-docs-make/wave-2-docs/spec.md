---
id: wave.fix-linux-install-docs-make.2-docs
---

# wave-2-docs

## Summary

Rewrite the four Linux install texts around the one-liner and add the missing Claude Code plugin step

## Network

- Parent: [[spec.fix-linux-install-docs-make]]
- Depends on: [[wave.fix-linux-install-docs-make.1-installer]]

## Tasks

- [ ] TUTORIAL-LINUX.md: make the one-line curl install the primary route in sections 2 and 3, and keep the manual .deb + install.sh route (with the sha256 check) as the documented alternative for whoever wants to verify before installing
- [ ] TUTORIAL-LINUX.md: add the missing plugin step after `mustard init` — `/plugin marketplace add rubensrpj/mustard` then `/plugin install mustard@mustard-local` — and delete the claim in section 5 that no extra step is necessary; add a troubleshooting entry for 'Plugin "mustard" not found in any marketplace'
- [ ] RELEASE-BODY.md: the Linux line of the quick summary becomes the one-liner; the asset table stays, since the manual route still needs it
- [ ] README.md and README.en.md: the Linux row of the install table leads with the one-liner, and the placeholder in the plugin step becomes the concrete `rubensrpj/mustard`
- [ ] Keep every install text in the language it is already written in — README.en.md stays English, the rest stay as they are

## Files

- `packaging/installer/TUTORIAL-LINUX.md`
- `packaging/installer/RELEASE-BODY.md`
- `README.md`
- `README.en.md`
