; ============================================================================
; hooks.nsh — everything the Windows installer does to the machine beyond
; copying files: the user Path, the templates variable and the Start Menu
; shortcut. Included by packaging/windows/mustard.nsi, which decides when each
; macro runs.
;
; Installed layout (mustard.nsi writes it; this file depends on it):
;
;   $INSTDIR\mustard-cli\        scan.exe, mustard*.exe, rtk.exe,
;                                mustard-dashboard.exe
;   $INSTDIR\mustard-cli\dist\   the built React assets
;   $INSTDIR\mustard-templates\  the payload `mustard init` copies
;
; THE FOLDER NAME `mustard-cli` IS A CONTRACT WITH mustard.nsi. It used to be a
; contract with the old desktop-app bundler's Windows config instead, and the two
; disagreed until 0.1.52:
; the bundler copied the binaries to $INSTDIR\mustard-cli while this file sent a
; `resources\mustard-cli` that never existed to the Path, so NOTHING new entered
; the Path. Anyone with an older install kept being answered by it — that is how
; a machine updated to 0.1.52 went on reporting 0.1.47 (the number is compiled
; into the executable: whatever answers 0.1.47 IS a 0.1.47 binary). Anyone with
; no prior install got no command at all. When touching either file, check the
; pair: both must name the SAME folder.
;
; Templates: `mustard_cli::resolve_templates_dir` tries MUSTARD_TEMPLATES_DIR
; FIRST, so pointing that variable at the installed folder is enough for both
; the CLI in a terminal and the dashboard server, with no reliance on relative
; layout. That resolution tests `is_dir()` before accepting the variable, so a
; variable aimed at a missing folder degrades to the layout beside the
; executable instead of breaking — which is why the wrong path above raised no
; error at all.
;
; SHORTCUT: it starts a SERVER, not a window. `mustard-dashboard.exe` binds
; 127.0.0.1:7777, prints the URL and opens the browser itself when there is a
; graphical session. The console window that stays open IS the server — closing
; it stops serving. "Start in" is the user profile because the scan root is the
; directory the server was started from; a narrower root is `--root DIR` from a
; terminal.
;
; POR QUE O PATH É EDITADO PELO POWERSHELL, NUNCA PELO NSIS: o NSIS trunca
; toda string no seu limite de compilação (1024 na build clássica, 8192 na de
; strings longas) e ReadRegStr NÃO AVISA — um Path de usuário maior que o
; limite volta truncado ou vazio, o fluxo antigo concluía "Path vazio!" e o
; WriteRegExpandStr seguinte gravava só a pasta do Mustard por cima do Path
; INTEIRO do usuário. Não é teoria: aconteceu em campo (sessão de 2026-08-26 —
; o Path de usuário de um dev foi apagado, levando junto o próprio Claude
; Code). O valor do Path portanto nunca pode passar por uma variável NSIS.
; [Environment]::Get/SetEnvironmentVariable não tem limite, compara sem
; diferenciar maiúsculas, e o Set já difunde a mudança de ambiente sozinho.
;
; A edição também é IDEMPOTENTE, o que o fluxo antigo não era: antes de anexar,
; toda entrada terminada em \mustard-cli é removida — a desta versão (não
; duplica em upgrade), as de $INSTDIR antigos (não deixa entrada morta apontando
; para pasta desinstalada) e as entradas MORTAS que as versões até 0.1.52
; gravaram sob a subpasta errada. O sufixo casado é o genérico `*\mustard-cli`
; justamente para alcançar as três de uma vez.
;
; Fail-open: se o powershell.exe faltar ou falhar, o Path fica como está — o
; usuário adiciona a pasta à mão, e nada dele é perdido. O código de saída é
; lido e descartado por isso mesmo.
;
; POSTUNINSTALL: remove a variável de templates e, agora com remoção por
; comparação exata de sufixo no PowerShell (o motivo de antes não remover era
; a fragilidade de fazê-lo por substring em NSIS), tira do Path só as
; entradas \mustard-cli.
;
; MUSTARD_TEMPLATES_DIR continua escrita pelo NSIS: é um valor curto que o
; instalador acabou de criar — o perigo do limite não a alcança.
;
; Notifica o sistema com WM_SETTINGCHANGE para o ambiente atualizar sem logoff.
; ============================================================================

!include "WinMessages.nsh"

!macro NSIS_HOOK_POSTINSTALL
  WriteRegExpandStr HKCU "Environment" "MUSTARD_TEMPLATES_DIR" "$INSTDIR\mustard-templates"

  ; Path de usuário: lido, filtrado e gravado inteiramente DENTRO do
  ; PowerShell — o valor nunca entra numa variável NSIS (ver cabeçalho).
  nsExec::ExecToLog `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$d = '$INSTDIR\mustard-cli'; $$p = [Environment]::GetEnvironmentVariable('Path', 'User'); $$k = @(); if ($$p) { $$k = @($$p -split ';' | Where-Object { $$_ -and ($$_ -notlike '*\mustard-cli') }) }; [Environment]::SetEnvironmentVariable('Path', (($$k + $$d) -join ';'), 'User')"`
  Pop $0

  ; Start Menu entry. SetOutPath decides the shortcut's working directory, and
  ; that directory is the dashboard's scan root — see the header.
  CreateDirectory "$SMPROGRAMS\Mustard"
  SetOutPath "$PROFILE"
  CreateShortCut "$SMPROGRAMS\Mustard\Mustard Dashboard.lnk" "$INSTDIR\mustard-cli\mustard-dashboard.exe" "" "$INSTDIR\mustard-cli\mustard-dashboard.exe" 0 SW_SHOWNORMAL "" "Serve o Mustard Dashboard em http://127.0.0.1:7777 e abre o navegador"
  SetOutPath "$INSTDIR"

  ; --- o passo do plugin ----------------------------------------------------
  ; O .exe atualiza a CÓPIA DO SISTEMA ($INSTDIR\mustard-cli). O Claude Code
  ; executa a CÓPIA DO PLUGIN (~\.claude\plugins\cache\…), porque o plugin
  ; prepende o bin/ dele ao PATH. Atualizar só a primeira é o que deixava uma
  ; máquina com 0.1.55 instalada desenhando 0.1.47 na barra de status.
  ;
  ; O script vai junto para $INSTDIR\mustard-cli e é chamado dali. Sem
  ; rebaixamento de usuário: o NSIS já roda como a própria pessoa e grava em
  ; HKCU, então o `claude` enxerga o ~\.claude certo — ao contrário do .deb e do
  ; .pkg, que rodam como root e por isso precisam do rebaixamento no irmão POSIX.
  ;
  ; Fail-open como todo o resto deste arquivo: o código de saída é lido e
  ; descartado. O Mustard já está instalado quando esta linha roda, e derrubar o
  ; instalador aqui faria a pessoa concluir que nada foi instalado.
  SetOutPath "$INSTDIR\mustard-cli"
  File "${__FILEDIR__}\..\installer\plugin-step.ps1"
  SetOutPath "$INSTDIR"
  nsExec::ExecToLog `powershell -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\mustard-cli\plugin-step.ps1"`
  Pop $0

  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Environment" "MUSTARD_TEMPLATES_DIR"

  Delete "$SMPROGRAMS\Mustard\Mustard Dashboard.lnk"
  RMDir "$SMPROGRAMS\Mustard"

  ; Remove só as entradas \mustard-cli; todo o resto do Path passa
  ; intocado pelo mesmo caminho sem limite do POSTINSTALL.
  nsExec::ExecToLog `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$p = [Environment]::GetEnvironmentVariable('Path', 'User'); if ($$p) { $$k = @($$p -split ';' | Where-Object { $$_ -and ($$_ -notlike '*\mustard-cli') }); [Environment]::SetEnvironmentVariable('Path', ($$k -join ';'), 'User') }"`
  Pop $0

  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend
