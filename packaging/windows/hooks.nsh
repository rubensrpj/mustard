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
; POSTINSTALL: aponta MUSTARD_TEMPLATES_DIR e põe a pasta dos binários do CLI
; no PATH do usuário (HKCU — não exige privilégio de administrador).
; POSTUNINSTALL: remove a variável (o PATH é deixado intacto de propósito —
; remover uma entrada de PATH por substring em NSIS é frágil e arriscado).
;
; Notifica o sistema com WM_SETTINGCHANGE para o ambiente atualizar sem logoff.
; ============================================================================

!include "WinMessages.nsh"

!macro NSIS_HOOK_POSTINSTALL
  WriteRegExpandStr HKCU "Environment" "MUSTARD_TEMPLATES_DIR" "$INSTDIR\resources\mustard-templates"

  ReadRegStr $R1 HKCU "Environment" "Path"
  StrCmp $R1 "" mustard_path_empty mustard_path_append
  mustard_path_empty:
    WriteRegExpandStr HKCU "Environment" "Path" "$INSTDIR\resources\mustard-cli"
    Goto mustard_path_done
  mustard_path_append:
    WriteRegExpandStr HKCU "Environment" "Path" "$R1;$INSTDIR\resources\mustard-cli"
  mustard_path_done:

  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Environment" "MUSTARD_TEMPLATES_DIR"
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend
