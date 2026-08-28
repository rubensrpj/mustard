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
#
# INSTALAR NÃO BASTA. Instalar deixa o plugin DESLIGADO e SEM binários, e cada
# um desses estados tranca o remédio do outro — medido nesta mesma máquina
# Windows em 2026-08-28: três instalações seguidas do .exe e umas duas horas de
# diagnóstico à mão. Ligar mora em `enabledPlugins`, que instalador nenhum
# escrevia; baixar mora no `mustard-boot`, que é um HOOK e não roda com o plugin
# desligado. Daí os dois passos depois do install, LIGAR e BAIXAR, gêmeos linha
# a linha dos do plugin-step.sh — e há um teste de paridade que reprova quando
# um dos dois ganha um passo e o outro não.
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

# --- liga o plugin -----------------------------------------------------------
# `claude plugin install` NÃO liga o que instalou. Sem esta função o Mustard
# fica instalado e INERTE: a barra desenha a versão, e mais nada acontece.
# Sem `-Scope`: o `enable` descobre sozinho o escopo em que o plugin foi
# instalado, e um escopo dito errado ligaria o plugin em outro lugar.
function Enable-Plugin {
    Write-Host "==> Ligando o plugin $Plugin..."
    & claude plugin enable $Plugin 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) {
        Write-Host '    Plugin ligado.'
    } else {
        # Já ligado também responde erro, e nesse caso não há nada a fazer.
        # Como os dois casos são indistinguíveis daqui, o aviso diz os dois —
        # calar seria pior: um plugin que ficou desligado é justamente o defeito
        # que este passo existe para acabar.
        Write-Host "aviso: claude plugin enable $Plugin respondeu erro - ou ja"
        Write-Host '       estava ligado, ou nao ligou. Se os comandos /mustard:*'
        Write-Host '       nao aparecerem, confira com claude plugin list.'
    }
}

# --- onde o Claude Code pôs o plugin ----------------------------------------
# O caminho vem de `claude plugin list --json`, interface PÚBLICA, e não de uma
# varredura em ~\.claude\plugins\cache — o layout daquele cache é interno, e
# este arquivo já se recusa a depender dele para instalar. Sem saída, sem JSON
# ou sem entrada do Mustard, devolve string vazia e quem chama decide.
function Get-PluginPath {
    try {
        $bruto = (& claude plugin list --json 2>$null | Out-String)
        if (-not $bruto.Trim()) { return '' }
        $lista = $bruto | ConvertFrom-Json
        $entrada = @($lista) | Where-Object { $_.id -like 'mustard@*' } | Select-Object -First 1
        if ($entrada -and $entrada.installPath) { return [string]$entrada.installPath }
    } catch {
        # JSON ausente ou de outro formato: quem chama imprime o aviso.
    }
    return ''
}

# --- dispara a descida dos binários -----------------------------------------
# O `--version` é de propósito: o `mustard-boot` baixa o que falta e entrega a
# invocação ao binário, então pedir a versão custa um comando e ainda IMPRIME a
# prova de que a descida funcionou. Sem argumento nenhum o `mustard-rt` sai com
# erro de uso, e o passo acusaria falha onde não houve.
function Start-BinaryDownload {
    $dir = Get-PluginPath
    $boot = ''
    if ($dir) { $boot = Join-Path $dir 'bin\mustard-boot.cmd' }
    if (-not $boot -or -not (Test-Path -LiteralPath $boot)) {
        Write-Host 'aviso: nao localizei o mustard-boot do plugin, entao os binarios'
        Write-Host '       so descem na primeira sessao do Claude Code.'
        return
    }

    Write-Host '==> Baixando os binarios do plugin...'
    & cmd /c "`"$boot`" --version"
    if ($LASTEXITCODE -ne 0) {
        Write-Host 'aviso: a descida dos binarios nao concluiu - a primeira sessao'
        Write-Host '       do Claude Code tenta de novo.'
    }
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
    Enable-Plugin
    Start-BinaryDownload
    Write-Host '    FECHE E ABRA o Claude Code para a nova versao entrar.'
} else {
    Write-Host "aviso: claude plugin $acao $Plugin nao concluiu."
    Show-ManualSteps
}

exit 0
