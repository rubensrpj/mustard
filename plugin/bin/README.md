# Native binaries

The release workflow supplies `mustard`, `mustard-rt` and `scan` (with `.exe`
on Windows). Executables are not committed. The plugin's POSIX and Windows
bootstrap scripts download the matching release when these files are missing;
hooks and Mods then call the native runtime directly. The runtime lives at
`${CLAUDE_PLUGIN_ROOT}/bin/mustard-rt` (with `.exe` on Windows).

The workspace and plugin manifest currently declare version `0.2.7`. A release
must stamp the same version in the binaries and manifest. A development build
stays in the isolated checkout; it never overwrites a personal installation.
