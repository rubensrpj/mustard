; ============================================================================
; hooks.nsh — hooks do instalador NSIS do Tauri (Tauri 2) para o Mustard.
;
; O instalador do Mustard Dashboard embute também o CLI: via bundle.resources
; (ver packaging/windows/tauri.windows.json) os binários do CLI e os templates
; são copiados para dentro da pasta de instalação:
;
;   $INSTDIR\mustard-cli\        scan.exe, mustard*.exe, rtk.exe
;   $INSTDIR\mustard-templates\  a carga do `mustard init`
;
; POR QUE ESSES CAMINHOS NÃO TÊM UMA SUBPASTA `resources\`: o destino declarado
; em bundle.resources é relativo à RAIZ de $INSTDIR no bundler NSIS. A subpasta
; `resources` é layout do `.app` do macOS (Contents/Resources), não do Windows.
; Este arquivo escreveu o caminho errado até 0.1.52 e o efeito era mudo: o
; instalador copiava os binários para $INSTDIR\mustard-cli e mandava para o Path
; uma pasta que nunca existiu, então NADA de novo entrava no Path. Quem já tinha
; uma instalação antiga continuava sendo respondido por ela — foi assim que uma
; máquina atualizada para 0.1.52 seguiu dizendo 0.1.47 (o número é gravado
; dentro do executável em tempo de compilação: quem responde 0.1.47 É um binário
; 0.1.47). Quem não tinha nada ficava sem comando algum. Ao mexer aqui, confira
; o par contra tauri.windows.json: os dois têm de nomear a MESMA pasta.
;
; O Dashboard.exe fica em $INSTDIR. Como a resolução de templates do Mustard
; (mustard_cli::resolve_templates_dir) tenta MUSTARD_TEMPLATES_DIR PRIMEIRO,
; apontar essa variável basta para que TANTO o CLI no terminal QUANTO o
; Dashboard encontrem os templates — sem depender de layout relativo. Essa
; resolução testa `is_dir()` antes de aceitar a variável, então uma variável
; apontando para pasta inexistente degrada para o layout ao lado do executável
; em vez de quebrar: por isso o caminho errado acima não deu erro nenhum.
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

  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Environment" "MUSTARD_TEMPLATES_DIR"

  ; Remove só as entradas \mustard-cli; todo o resto do Path passa
  ; intocado pelo mesmo caminho sem limite do POSTINSTALL.
  nsExec::ExecToLog `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$p = [Environment]::GetEnvironmentVariable('Path', 'User'); if ($$p) { $$k = @($$p -split ';' | Where-Object { $$_ -and ($$_ -notlike '*\mustard-cli') }); [Environment]::SetEnvironmentVariable('Path', ($$k -join ';'), 'User') }"`
  Pop $0

  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend
