; ============================================================================
; hooks.nsh — hooks do instalador NSIS do Tauri (Tauri 2) para o Mustard.
;
; O instalador do Mustard Dashboard embute também o CLI: via bundle.resources
; (ver packaging/windows/tauri.windows.json) os binários do CLI e os templates
; são copiados para dentro da pasta de instalação:
;
;   $INSTDIR\resources\mustard-cli\        scan.exe, mustard*.exe, rtk.exe
;   $INSTDIR\resources\mustard-templates\  a carga do `mustard init`
;
; O Dashboard.exe fica em $INSTDIR. Como a resolução de templates do Mustard
; (mustard_cli::resolve_templates_dir) tenta MUSTARD_TEMPLATES_DIR PRIMEIRO,
; apontar essa variável basta para que TANTO o CLI no terminal QUANTO o
; Dashboard encontrem os templates — sem depender de layout relativo.
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
; toda entrada terminada em \resources\mustard-cli é removida — a desta versão
; (não duplica em upgrade) e as de $INSTDIR antigos (não deixa entrada morta
; apontando para pasta desinstalada).
;
; Fail-open: se o powershell.exe faltar ou falhar, o Path fica como está — o
; usuário adiciona a pasta à mão, e nada dele é perdido. O código de saída é
; lido e descartado por isso mesmo.
;
; POSTUNINSTALL: remove a variável de templates e, agora com remoção por
; comparação exata de sufixo no PowerShell (o motivo de antes não remover era
; a fragilidade de fazê-lo por substring em NSIS), tira do Path só as
; entradas \resources\mustard-cli.
;
; MUSTARD_TEMPLATES_DIR continua escrita pelo NSIS: é um valor curto que o
; instalador acabou de criar — o perigo do limite não a alcança.
;
; Notifica o sistema com WM_SETTINGCHANGE para o ambiente atualizar sem logoff.
; ============================================================================

!include "WinMessages.nsh"

!macro NSIS_HOOK_POSTINSTALL
  WriteRegExpandStr HKCU "Environment" "MUSTARD_TEMPLATES_DIR" "$INSTDIR\resources\mustard-templates"

  ; Path de usuário: lido, filtrado e gravado inteiramente DENTRO do
  ; PowerShell — o valor nunca entra numa variável NSIS (ver cabeçalho).
  nsExec::ExecToLog `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$d = '$INSTDIR\resources\mustard-cli'; $$p = [Environment]::GetEnvironmentVariable('Path', 'User'); $$k = @(); if ($$p) { $$k = @($$p -split ';' | Where-Object { $$_ -and ($$_ -notlike '*\resources\mustard-cli') }) }; [Environment]::SetEnvironmentVariable('Path', (($$k + $$d) -join ';'), 'User')"`
  Pop $0

  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Environment" "MUSTARD_TEMPLATES_DIR"

  ; Remove só as entradas \resources\mustard-cli; todo o resto do Path passa
  ; intocado pelo mesmo caminho sem limite do POSTINSTALL.
  nsExec::ExecToLog `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$p = [Environment]::GetEnvironmentVariable('Path', 'User'); if ($$p) { $$k = @($$p -split ';' | Where-Object { $$_ -and ($$_ -notlike '*\resources\mustard-cli') }); [Environment]::SetEnvironmentVariable('Path', ($$k -join ';'), 'User') }"`
  Pop $0

  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend
