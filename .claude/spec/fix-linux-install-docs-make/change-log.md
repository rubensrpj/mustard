# Change Log — fix-linux-install-docs-make

_Solicitações registradas automaticamente durante o pipeline (mid-spec). O `spec.md` (narrativa congelada) NÃO é alterado; dobre o que muda comportamento em `## Acceptance Criteria` e rode o QA de novo._

- **2026-08-17T12:23:59.087Z** _(Execute)_ — segue
- **2026-08-17T12:31:42.228Z** _(Execute)_ — **Instruction:** packaging/installer/README.txt is a FIFTH install text, versioned and shipped in the tar.gz bundle, and it still teaches only the manual './install.sh next to the .deb' route (lines 26-28) with no plugin/marketplace step. Wave 2 must update it the same way as the other four: lead with the one-line curl install, keep the manual route as the alternative, and add the concrete '/plugin marketplace add rubensrpj/mustard' + '/plugin install mustard@mustard-local' step. Its Linux file listing must stop implying the .deb has to sit beside install.sh.
