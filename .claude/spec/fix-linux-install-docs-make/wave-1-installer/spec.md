---
id: wave.fix-linux-install-docs-make.1-installer
---

# wave-1-installer

## Summary

Make install.sh fetch the .deb itself, survive being piped into sh, and expose a --dry-run that resolves without installing

## Network

- Parent: [[spec.fix-linux-install-docs-make]]

## Tasks

- [ ] Resolve the package without depending on $0: keep a .deb sitting beside the script as the preferred source, but stop treating its absence as fatal and stop assuming $0 is a real path (a piped run has none)
- [ ] When no .deb is found beside the script, resolve the newest release tag by following the redirect of https://github.com/rubensrpj/mustard/releases/latest, download mustard_<version>_amd64.deb into a mktemp -d directory, and hand THAT path to apt-get
- [ ] Accept an explicit version through the MUSTARD_VERSION environment variable, falling back to the resolved latest tag; fail with a clear message when neither curl nor wget is present
- [ ] Add --dry-run: resolve the package (local file or release URL), print what would be installed, exit 0 without calling apt-get. It must exit 0 offline too, naming the URL it would have used, so the criterion never depends on the network
- [ ] Remove the temporary download directory on exit, including on failure
- [ ] Keep the existing optional project-path argument (which runs `mustard init`) working, and keep it distinguishable from the new flag
- [ ] Keep the script POSIX sh — it is executed by /bin/sh on Ubuntu, not bash

## Files

- `packaging/installer/install.sh`
