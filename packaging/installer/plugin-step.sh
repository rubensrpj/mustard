#!/usr/bin/env sh
# ============================================================================
# plugin-step.sh — o passo que faltava no instalador: atualizar a CÓPIA DO
# PLUGIN.
#
# Existem DUAS cópias do mesmo executável numa máquina com Mustard:
#
#   cópia do SISTEMA ... /usr/bin/mustard-rt (.deb), /usr/local/bin (.pkg),
#                        $INSTDIR\mustard-cli (.exe) — o que o instalador põe
#   cópia do PLUGIN .... ~/.claude/plugins/cache/<marketplace>/mustard/<v>/bin/
#                        — o que o Claude Code REALMENTE executa
#
# O plugin prepende o `bin/` dele ao PATH, então dentro do Claude Code a cópia
# do sistema nunca é alcançada: hooks, comandos e barra de status saem todos da
# cópia do plugin. Instalar o pacote e parar aí deixa a máquina rodando a versão
# velha indefinidamente — o instalador se limitava a IMPRIMIR duas linhas
# pedindo que a pessoa atualizasse o plugin à mão, e enquanto ninguém digitasse,
# nada acontecia (campo, 2026-08-28: 0.1.55 instalada nos três sistemas, plugin
# parado em 0.1.54 no Linux e 0.1.47 no Windows).
#
# Este script fecha isso pela linha de comando PÚBLICA do Claude Code
# (`claude plugin …`). Escrever direto no cache do plugin foi recusado de
# propósito: aquele layout é interno e pode mudar sem aviso.
#
# INSTALAR NÃO BASTA, e é por isso que este arquivo não termina no `install`.
# Instalar deixa o plugin DESLIGADO e SEM binários, e cada um desses dois
# estados tranca o remédio do outro (campo, 2026-08-28: três instalações
# seguidas do .exe, ~2h de diagnóstico à mão):
#
#   - desligado: o Claude Code guarda ligado/desligado em `enabledPlugins`, e
#     nenhum instalador jamais escreveu ali. Plugin desligado = zero hooks,
#     zero comandos /mustard:*.
#   - sem binários: `plugin/bin/*` são artefatos de build e nunca entram no
#     git. Quem os baixa é o `mustard-boot` — que é um HOOK, e hook não roda
#     com o plugin desligado.
#
# Daí os dois passos depois do install: LIGAR e BAIXAR. Nesta ordem, e aqui
# dentro, para o instalador terminar com a máquina pronta em vez de deixar a
# primeira sessão descobrir que ela não está.
#
# FAIL-OPEN, sempre. Um instalador não pode falhar porque o passo do plugin
# falhou: o pacote JÁ está instalado quando chegamos aqui, e um exit != 0 faria
# a pessoa concluir que nada foi instalado. Todo caminho termina em `exit 0`, e
# o que não deu certo vira instrução impressa — que é exatamente o que o
# instalador fazia antes, agora como exceção em vez de regra.
#
# ROOT: o `apt` e o `.pkg` rodam como root, mas o plugin mora no `~/.claude` de
# UMA pessoa. Rodar `claude` como root instalaria o plugin para o root, e o
# `~/.claude` da pessoa continuaria intocado — a mesma classe de erro que o
# `mustard init` já resolve com SUDO_USER. Quando dá para saber quem chamou, o
# passo se rebaixa para essa pessoa; quando não dá, ele se RECUSA a adivinhar e
# imprime as instruções, porque instalar no lugar errado é pior que não
# instalar.
# ============================================================================
set -u

MARKETPLACE_REPO="rubensrpj/mustard"
MARKETPLACE_NAME="mustard-local"
PLUGIN="mustard@$MARKETPLACE_NAME"

instrucoes_manuais() {
  echo
  echo "    Falta atualizar o plugin do Claude Code — é ele que traz os comandos"
  echo "    /mustard:*, os hooks e o MCP de memória, e é a cópia que o Claude Code"
  echo "    de fato executa. Abra o Claude Code e digite estas linhas DENTRO dele"
  echo "    (não são comandos de terminal):"
  echo "        /plugin marketplace add $MARKETPLACE_REPO"
  echo "        /plugin install $PLUGIN"
  echo "    Depois feche e abra o Claude Code para os hooks entrarem."
}

# --- liga o plugin -----------------------------------------------------------
# `claude plugin install` NÃO liga o que instalou. Sem esta linha o Mustard fica
# instalado e INERTE: a barra de status desenha a versão, e mais nada acontece.
# Sem `--scope`: o `enable` descobre sozinho o escopo em que o plugin foi
# instalado, e um escopo dito errado ligaria o plugin em outro lugar.
ligar_o_plugin() {
  echo "==> Ligando o plugin $PLUGIN…"
  if claude plugin enable "$PLUGIN" >/dev/null 2>&1; then
    echo "    Plugin ligado."
  else
    # Já ligado também responde erro, e nesse caso não há nada a fazer. Como os
    # dois casos são indistinguíveis daqui, o aviso diz os dois — calar seria
    # pior: um plugin que ficou desligado é justamente o defeito que este passo
    # existe para acabar.
    echo "aviso: 'claude plugin enable $PLUGIN' respondeu erro — ou já estava" >&2
    echo "       ligado, ou não ligou. Se os comandos /mustard:* não" >&2
    echo "       aparecerem, confira com 'claude plugin list'." >&2
  fi
}

# --- onde o Claude Code pôs o plugin ----------------------------------------
# O caminho vem de `claude plugin list --json`, interface PÚBLICA, e não de uma
# varredura em ~/.claude/plugins/cache — o layout daquele cache é interno, e
# este arquivo já se recusa a depender dele para instalar. Sem saída, sem JSON
# ou sem entrada do Mustard, a função devolve vazio e quem chama decide.
caminho_do_plugin() {
  claude plugin list --json 2>/dev/null | awk '
    /"id"[[:space:]]*:/ { meu = ($0 ~ /"mustard@/) }
    meu && match($0, /"installPath"[[:space:]]*:[[:space:]]*"[^"]*"/) {
      achado = substr($0, RSTART, RLENGTH)
      sub(/^"installPath"[[:space:]]*:[[:space:]]*"/, "", achado)
      sub(/"$/, "", achado)
      print achado
      exit
    }'
}

# --- dispara a descida dos binários -----------------------------------------
# O `--version` é de propósito: o `mustard-boot` baixa o que falta e entrega a
# invocação ao binário, então pedir a versão custa um comando e ainda IMPRIME a
# prova de que a descida funcionou. Sem argumento nenhum o `mustard-rt` sai com
# erro de uso, e o passo acusaria falha onde não houve.
baixar_os_binarios() {
  dir=$(caminho_do_plugin)
  if [ -z "$dir" ] || [ ! -f "$dir/bin/mustard-boot" ]; then
    echo "aviso: não localizei o mustard-boot do plugin, então os binários só" >&2
    echo "       descem na primeira sessão do Claude Code." >&2
    return
  fi

  echo "==> Baixando os binários do plugin…"
  # Invocado por `sh`, não pelo bit de execução: o passo funciona igual num
  # cache extraído sem permissões preservadas, e o `mustard-boot` já resolve o
  # próprio diretório a partir de `$0`.
  if ! sh "$dir/bin/mustard-boot" --version; then
    echo "aviso: a descida dos binários não concluiu — a primeira sessão do" >&2
    echo "       Claude Code tenta de novo." >&2
  fi
}

# --- quem deve rodar o `claude` ---------------------------------------------
# A variável abaixo marca que já nos rebaixamos uma vez; sem ela, o `sh "$0"`
# do sudo entraria aqui de novo e o rebaixamento se repetiria para sempre.
if [ "$(id -u)" -eq 0 ] && [ -z "${MUSTARD_PLUGIN_STEP_DEESCALATED:-}" ]; then
  dono=""
  if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ]; then
    dono="$SUDO_USER"
  elif [ "$(uname -s)" = "Darwin" ]; then
    # No .pkg não existe SUDO_USER: o instalador gráfico roda como root direto.
    # Quem está logado na tela é a melhor resposta disponível.
    console=$(stat -f%Su /dev/console 2>/dev/null || echo "")
    if [ -n "$console" ] && [ "$console" != "root" ]; then dono="$console"; fi
  fi

  if [ -n "$dono" ] && command -v sudo >/dev/null 2>&1; then
    echo "==> Passo do plugin: rodando como $dono, não como root."
    # -H para o HOME ser o da pessoa: é o HOME que decide qual ~/.claude o
    # `claude` enxerga, e é o ponto inteiro deste rebaixamento.
    # O resultado do sudo É lido. Sem este `if`, um sudo que falhasse (regra de
    # sudoers, conta sem shell, o script sem permissão de leitura para o dono)
    # sairia daqui com 0 e em SILÊNCIO: o instalador diria "pronto" e o plugin
    # continuaria na versão velha, que é exatamente o defeito que este arquivo
    # existe para acabar.
    if ! sudo -H -u "$dono" \
         env MUSTARD_PLUGIN_STEP_DEESCALATED=1 sh "$0" "$@"; then
      echo "aviso: não consegui rodar o passo do plugin como $dono." >&2
      instrucoes_manuais
    fi
    exit 0
  fi

  echo "aviso: o passo do plugin está rodando como root e não descobriu quem" >&2
  echo "       chamou — instalar o plugin aqui o poria no ~/.claude do root." >&2
  instrucoes_manuais
  exit 0
fi

# --- o Claude Code está na máquina? -----------------------------------------
if ! command -v claude >/dev/null 2>&1; then
  echo "aviso: não achei o comando 'claude' no PATH, então não dá para atualizar" >&2
  echo "       o plugin daqui. O Mustard em si ESTÁ instalado." >&2
  instrucoes_manuais
  exit 0
fi

# --- registra o marketplace (idempotente) -----------------------------------
# Já registrado devolve erro, e esse erro não é problema nenhum: o que importa é
# o marketplace existir depois desta linha, não esta linha ter sido a criadora.
echo "==> Registrando o marketplace do Mustard no Claude Code…"
if ! claude plugin marketplace add "$MARKETPLACE_REPO" >/dev/null 2>&1; then
  echo "    (já estava registrado — seguindo)"
fi

# --- instala ou atualiza -----------------------------------------------------
if claude plugin list 2>/dev/null | grep -q "mustard@"; then
  acao="update"
  echo "==> Atualizando o plugin $PLUGIN…"
else
  acao="install"
  echo "==> Instalando o plugin $PLUGIN…"
fi

if claude plugin "$acao" "$PLUGIN"; then
  echo "==> Plugin: $acao concluído."
  ligar_o_plugin
  baixar_os_binarios
  echo "    FECHE E ABRA o Claude Code para a nova versão entrar."
else
  echo "aviso: 'claude plugin $acao $PLUGIN' não concluiu." >&2
  instrucoes_manuais
fi

exit 0
