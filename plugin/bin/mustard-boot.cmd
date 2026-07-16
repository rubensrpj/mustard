@echo off
rem mustard-boot.cmd — first-run bootstrap for the Mustard plugin binaries
rem (Windows twin of ./mustard-boot; hooks.json calls the extensionless name
rem and cmd resolves this file via PATHEXT). Fetches mustard-bins-<v>-windows
rem from the GitHub Release matching plugin.json's version when mustard-rt.exe
rem is missing or version-stamped differently, then delegates. Every failure
rem exits 0: a hook must never wedge the session. A locally built bin/ has no
rem .version stamp and is never overwritten.
setlocal
set "DIR=%~dp0"
set "RT=%DIR%mustard-rt.exe"
set "MANIFEST=%DIR%..\.claude-plugin\plugin.json"
set "STAMP=%DIR%.version"

set "VER="
for /f "usebackq delims=" %%v in (`powershell -NoProfile -Command "try { (Get-Content -Raw '%MANIFEST%' | ConvertFrom-Json).version } catch {}"`) do set "VER=%%v"

set "NEED=0"
if not exist "%RT%" set "NEED=1"
if not exist "%STAMP%" goto :decided
if "%VER%"=="" goto :decided
set /p CUR=<"%STAMP%"
if not "%CUR%"=="%VER%" set "NEED=1"
:decided

if not "%NEED%"=="1" goto :run
if "%VER%"=="" goto :run

set "URL=https://github.com/rubensrpj/mustard/releases/download/v%VER%/mustard-bins-%VER%-windows-x64.zip"
set "ZIP=%TEMP%\mustard-bins-%VER%.zip"
echo [mustard-boot] fetching plugin binaries v%VER% (first run) 1>&2
curl -fsSL "%URL%" -o "%ZIP%"
if errorlevel 1 goto :fetchfail
tar -xf "%ZIP%" -C "%DIR%"
if errorlevel 1 goto :fetchfail
<nul set /p="%VER%">"%STAMP%"
del /q "%ZIP%" 2>nul
goto :run

:fetchfail
echo [mustard-boot] download failed (%URL%) — mustard hooks stay dormant this session 1>&2
del /q "%ZIP%" 2>nul

:run
if exist "%RT%" (
  "%RT%" %*
  exit /b 0
)
exit /b 0
