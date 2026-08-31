@echo off
rem mustard-boot.cmd — first-run bootstrap for the Mustard plugin binaries
rem (Windows twin of ./mustard-boot; hooks.json calls the extensionless name
rem and cmd resolves this file via PATHEXT). Fetches mustard-bins-<v>-windows
rem from the GitHub Release matching plugin.json's version when mustard-rt.exe
rem is missing or version-stamped differently, then delegates. Every failure
rem exits 0: a hook must never wedge the session. A locally built bin/ has no
rem .version stamp and is never overwritten.
rem
rem TRAILING BACKSLASH — the defect this file carried until 0.1.52, and the one
rem rule to keep in mind when editing it. %~dp0 ALWAYS ends in a backslash, and
rem on Windows a backslash at the end of a QUOTED path escapes the closing quote
rem instead of closing the citation. Feeding tar's -C flag with %DIR% therefore
rem handed it a path with a quote INSIDE it, and extraction failed on every
rem Windows machine, from
rem the very first session, in silence: measured in the field 2026-08-27 as
rem `tar: could not chdir to 'C:\Users\...\0.1.52\bin"'`, with bin/ left holding
rem nothing but this script. %DEST% is %DIR% minus that last character and is the
rem ONLY form the -C may ever receive. %DIR% keeps its backslash because RT,
rem MANIFEST and STAMP concatenate onto it.
rem
rem DOWNLOADING AND EXTRACTING FAIL SEPARATELY, so they say separate things. The
rem single shared "download failed (%URL%)" sent a diagnosis session hunting a
rem network that was perfectly fine — the download had succeeded and only the
rem extraction had not. Never merge the two labels back into one.
rem
rem WHO PAYS FOR THE DOWNLOAD — and it is not "whoever gets here first". Every
rem hook comes through this script, and the harness gives each its own budget
rem (`timeout` in plugin/hooks/hooks.json); the tightest is 15 seconds, and that
rem is not only SessionEnd — the `clear|compact|resume|fork` arm of SessionStart
rem carries 15 too. A download has no ceiling the caller can see, so a
rem 15-second hook that opens one is not unlucky, it is cancelled: the harness
rem kills it and prints `Hook cancelled` (field, this file, Windows, 2026-08-28
rem — SessionEnd right after the plugin changed version, so nothing after it ran
rem and the session's OTEL collector outlived the project). Only `on
rem SessionStart` and NON-hook callers (the installer warms the plugin up with
rem `mustard-boot.cmd --version`) may fetch; every other trigger stays dormant,
rem which is the promise the header above already makes. The gate is the trigger
rem on argv, not an environment variable — `VAR=1 prog` is POSIX syntax cmd.exe
rem does not parse, and the twins have to answer the same way.
setlocal
set "DIR=%~dp0"
set "DEST=%DIR:~0,-1%"
set "RT=%DIR%mustard-rt.exe"
set "MANIFEST=%DIR%..\.claude-plugin\plugin.json"
set "STAMP=%DIR%.version"

rem THE VERSION IS READ IN PLAIN CMD, and the reason is the clock. Asking a
rem scripting host to parse the manifest spawned a whole extra process on EVERY
rem hook — the POSIX twin does the same job with one `sed` and no new process —
rem and this file sits in front of PreToolUse, which fires on every tool call.
rem
rem No external tool either, not even findstr: `for /f` reads the manifest
rem itself, cutting each line on colon, comma and space (the space is last in
rem `delims=`, which is the only place cmd accepts it). `  "version": "0.1.58",`
rem therefore yields exactly two tokens, `"version"` and `"0.1.58"` — each one
rem wholly quoted, so `%%~a` and `%%~b` unquote them for free and there is
rem nothing left to strip. Every other line fails the key test and costs one
rem comparison.
rem
rem This reads the manifest as one key per line, which is how it is committed
rem and how the release ships it: the PATCH bump in
rem .github/workflows/bump-on-main.yml is a `sed` on that very line, so the shape
rem survives every release. Any other shape leaves VER empty, and an empty VER
rem means "do nothing" everywhere below — dormant, and as of :noversion it also
rem SAYS so instead of going quiet.
rem
rem tests/plugin_prose_matches_shipped_behaviour.rs models the tokenisation of
rem the line below against the real manifest, and on Windows asks cmd.exe
rem itself. That second half replaces a claim this block used to make — that no
rem runner here could execute the file — which was false, and cost six releases.
rem
rem NO ILLUSTRATIVE PERCENT-TILDE IN THIS FILE, comment or code. cmd expands
rem percent sequences BEFORE it notices a line is a rem, so one that names no
rem argument ABORTS the whole file, exit 255. That is how 0.1.59 through 0.1.61
rem shipped dormant on every Windows machine: this very comment carried the
rem copy-pasteable one-liner, which now lives beside its model in that test
rem file, where a percent sign is inert prose. Plain variables are fine. Which
rem shapes are not is stated by, and only by,
rem every_batch_file_carries_no_percent_sequence_cmd_will_refuse.
set "VER="
for /f "usebackq tokens=1,2 delims=:, " %%a in ("%MANIFEST%") do if not defined VER if /i "%%~a"=="version" set "VER=%%~b"

set "NEED=0"
if not exist "%RT%" set "NEED=1"
if not exist "%STAMP%" goto :decided
if "%VER%"=="" goto :decided
set /p CUR=<"%STAMP%"
if not "%CUR%"=="%VER%" set "NEED=1"
:decided

rem The fetch gate, twin of the POSIX one: a hook arrives as `on <Trigger>`, and
rem every trigger but SessionStart runs on a budget too short to survive a
rem download. Dropping NEED rather than jumping straight to :run is what lets a
rem binary that is merely stamped for another version still be handed the
rem invocation at :run — an old harness beats a dormant one. Anything not
rem opening with `on` is not a hook (the installer's `--version` warm-up) and
rem keeps the right to fetch.
if /i "%~1"=="on" if /i not "%~2"=="SessionStart" set "NEED=0"

if not "%NEED%"=="1" goto :run
if "%VER%"=="" goto :noversion

set "URL=https://github.com/rubensrpj/mustard/releases/download/v%VER%/mustard-bins-%VER%-windows-x64.zip"
set "ZIP=%TEMP%\mustard-bins-%VER%.zip"
echo [mustard-boot] fetching plugin binaries v%VER% (first run) 1>&2
rem `--max-time 10` is the ceiling, and the number is not free: the tightest
rem budget that can still reach this line is 15 seconds (the
rem `clear|compact|resume|fork` arm of SessionStart), and the unpack still has to
rem fit after it. The case it covers is a connection that HANGS rather than
rem refuses — captive portal, a proxy that swallows the packet. Keep the value
rem literal and in step with the POSIX twin.
curl -fsSL --max-time 10 "%URL%" -o "%ZIP%"
if errorlevel 1 goto :fetchfail
tar -xf "%ZIP%" -C "%DEST%"
if errorlevel 1 goto :extractfail
<nul set /p="%VER%">"%STAMP%"
del /q "%ZIP%" 2>nul
goto :run

:noversion
rem The one failure that is BOTH silent and total, and the reason it gets a
rem label of its own. NEED=1 says the binaries have to come down; an empty VER
rem names no release to get them from, so nothing downloads and nothing prints,
rem and a machine whose bin\ was never populated stays dormant forever with no
rem line on screen saying why. Nothing here starts a download — there is no URL
rem to build — but the operator is told, and told WHICH file to look at. Only a
rem caller that was going to download says anything, so the short-budget path
rem above stays as quiet as its own paragraph promises.
echo [mustard-boot] no "version" readable in "%MANIFEST%" — mustard hooks stay dormant this session 1>&2
goto :run

:fetchfail
echo [mustard-boot] download failed (%URL%) — mustard hooks stay dormant this session 1>&2
del /q "%ZIP%" 2>nul
goto :run

:extractfail
echo [mustard-boot] downloaded, but could not unpack into "%DEST%" — mustard hooks stay dormant this session 1>&2
del /q "%ZIP%" 2>nul

:run
if exist "%RT%" (
  "%RT%" %*
  exit /b 0
)
exit /b 0
