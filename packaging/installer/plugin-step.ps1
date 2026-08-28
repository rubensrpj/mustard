# ============================================================================
# plugin-step.ps1 — o gêmeo Windows de plugin-step.sh.
#
# Mesma razão de existir, escrita por inteiro lá: o instalador atualiza a cópia
# do sistema (INSTDIR\mustard-cli) e o Claude Code executa a cópia do PLUGIN
# (~\.claude\plugins\cache\...). Sem este passo, o .exe atualiza uma e o Mustard
# segue rodando a outra — no campo, uma máquina com 0.1.55 instalada continuava
# desenhando 0.1.47 na barra de status.
#
# Não há rebaixamento de root aqui, e isso é de propósito: o instalador NSIS
# grava tudo em HKCU e roda como a própria pessoa, então o `claude` já enxerga o
# ~\.claude certo. O irmão POSIX precisa do rebaixamento porque `apt` e `.pkg`
# rodam como root.
#
# FAIL-OPEN, igual ao irmão: `$ErrorActionPreference` fica em Continue e todo
# caminho termina em `exit 0`. O pacote JÁ está instalado quando este script
# roda; derrubar o instalador aqui faria a pessoa concluir que nada foi
# instalado.
# ============================================================================

$ErrorActionPreference = 'Continue'

$MarketplaceRepo = 'rubensrpj/mustard'
$MarketplaceName = 'mustard-local'
$Plugin = "mustard@$MarketplaceName"

function Show-ManualSteps {
    Write-Host ''
    Write-Host '    Falta atualizar o plugin do Claude Code - e ele que traz os comandos'
    Write-Host '    /mustard:*, os hooks e o MCP de memoria, e e a copia que o Claude Code'
    Write-Host '    de fato executa. Abra o Claude Code e digite estas linhas DENTRO dele'
    Write-Host '    (nao sao comandos de terminal):'
    Write-Host "        /plugin marketplace add $MarketplaceRepo"
    Write-Host "        /plugin install $Plugin"
    Write-Host '    Depois feche e abra o Claude Code para os hooks entrarem.'
}

# --- o Claude Code esta na maquina? -----------------------------------------
$claude = Get-Command claude -ErrorAction SilentlyContinue
if (-not $claude) {
    Write-Host 'aviso: nao achei o comando claude no PATH, entao nao da para atualizar'
    Write-Host '       o plugin daqui. O Mustard em si ESTA instalado.'
    Show-ManualSteps
    exit 0
}

# --- registra o marketplace (idempotente) -----------------------------------
# Ja registrado devolve erro, e esse erro nao e problema: o que importa e o
# marketplace existir depois desta linha, nao esta linha ter sido a criadora.
Write-Host '==> Registrando o marketplace do Mustard no Claude Code...'
& claude plugin marketplace add $MarketplaceRepo 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    Write-Host '    (ja estava registrado - seguindo)'
}

# --- instala ou atualiza -----------------------------------------------------
$listed = (& claude plugin list 2>&1 | Out-String)
if ($listed -match 'mustard@') {
    $acao = 'update'
    Write-Host "==> Atualizando o plugin $Plugin..."
} else {
    $acao = 'install'
    Write-Host "==> Instalando o plugin $Plugin..."
}

& claude plugin $acao $Plugin
if ($LASTEXITCODE -eq 0) {
    Write-Host "==> Plugin: $acao concluido."
    Write-Host '    FECHE E ABRA o Claude Code para a nova versao entrar.'
} else {
    Write-Host "aviso: claude plugin $acao $Plugin nao concluiu."
    Show-ManualSteps
}

exit 0
