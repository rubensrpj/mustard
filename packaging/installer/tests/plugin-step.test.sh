#!/usr/bin/env sh
# ============================================================================
# plugin-step.test.sh — exercita `packaging/installer/plugin-step.sh` nos
# caminhos que decidem se o instalador cumpre o que promete.
#
#   com-claude ............. há um `claude` no PATH: o passo TEM de registrar o
#                            marketplace e instalar-ou-atualizar o plugin. É o
#                            caminho normal.
#   sem-claude ............. não há `claude` nenhum: o passo TEM de sair com 0 e
#                            ensinar o caminho manual. Um instalador que morre
#                            aqui faria a pessoa concluir que o pacote não foi
#                            instalado — e ele foi.
#   liga-o-plugin .......... instalar NÃO basta: `claude plugin install` deixa o
#                            plugin desligado, e plugin desligado é zero hooks e
#                            zero comandos /mustard:*. O passo TEM de chamar
#                            `plugin enable` depois de instalar.
#   baixa-os-binarios ...... `plugin/bin/*` são artefatos de build e nunca entram
#                            no git. Quem os baixa é o `mustard-boot` — que é um
#                            HOOK, e hook não roda com o plugin recém-instalado.
#                            O passo TEM de disparar o `mustard-boot` ele mesmo,
#                            ali, sem hook nenhum no meio.
#   nao-trava-o-instalador . e TEM de largar esse `mustard-boot` se ele não
#                            voltar. O download não tem prazo próprio, e agora
#                            roda segurando o cadeado do apt.
#   paridade-ps1 ........... o gêmeo Windows não pode ficar para trás. Reprova
#                            quando um dos dois arquivos ganha um passo e o
#                            outro não.
#
# Os quatro casos do meio medem o mesmo ciclo por dentro: foi ele que, no campo
# em 2026-08-28, consumiu três instalações seguidas do .exe e uma tarde de
# diagnóstico à mão. Ligar sem baixar deixa o plugin dormente; baixar sem ligar
# não roda. Por isso cada metade tem o seu caso.
#
# O `claude` dos casos felizes é FALSO: um script que só anota o que lhe
# pediram. O teste não pode instalar plugin de verdade na máquina de quem o
# roda.
#
# Uso: sh packaging/installer/tests/plugin-step.test.sh <caso>
# ============================================================================
set -u

CASO="${1:-}"
AQUI=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) || exit 1
PASSO="$AQUI/../plugin-step.sh"
GEMEO="$AQUI/../plugin-step.ps1"

# Resolvido AGORA, com o PATH ainda inteiro: os casos abaixo estreitam o PATH do
# processo filho, e um `sh` procurado depois disso poderia não ser achado.
SH=$(command -v sh) || { echo "FALHA: não achei um 'sh'" >&2; exit 1; }

[ -f "$PASSO" ] || { echo "FALHA: não achei $PASSO" >&2; exit 1; }

TMP=$(mktemp -d) || exit 1
trap 'rm -rf "$TMP"' EXIT
trap 'rm -rf "$TMP"; exit 130' INT TERM HUP

mkdir -p "$TMP/bin" "$TMP/home"

falhar() { echo "FALHA: $1" >&2; exit 1; }

# Escreve o `claude` falso: cada invocação vira uma linha em $TMP/chamadas.txt e
# a saída é sempre bem-sucedida. Com um caminho em $1, ele também RESPONDE ao
# `plugin list --json` — na mesma forma de várias linhas que o comando real
# imprime — apontando o `installPath` para ali. Sem $1 ele cala nesse comando, e
# o passo conclui que precisa INSTALAR (e não atualizar).
escrever_claude_falso() {
  onde="${1:-}"
  cat > "$TMP/bin/claude" <<EOF
#!/bin/sh
echo "\$@" >> "$TMP/chamadas.txt"
if [ "\$*" = "plugin list --json" ] && [ -n "$onde" ]; then
  printf '%s\n' \
    '[' \
    '  {' \
    '    "id": "mustard@mustard-local",' \
    '    "version": "0.1.57",' \
    '    "installPath": "$onde"' \
    '  }' \
    ']'
fi
exit 0
EOF
  chmod +x "$TMP/bin/claude"
}

# Roda o passo do plugin num ambiente fechado: só o `claude` falso à frente do
# PATH e um HOME descartável, para nada tocar o ~/.claude de quem roda o teste.
# LIMITE vazio deixa o passo com o prazo de produção (120s).
LIMITE=""
rodar_o_passo() {
  PATH="$TMP/bin:$PATH" HOME="$TMP/home" \
    MUSTARD_PLUGIN_STEP_TIMEOUT="$LIMITE" "$SH" "$PASSO" 2>&1
}

case "$CASO" in
  com-claude)
    # `plugin list` responde vazio, então o passo deve concluir que precisa
    # INSTALAR — e o teste aceita install OU update, porque qual dos dois é
    # decisão do passo, não deste teste.
    escrever_claude_falso

    saida=$(rodar_o_passo)
    status=$?

    [ "$status" -eq 0 ] || falhar "o passo saiu com $status, e ele é fail-open:
$saida"
    [ -f "$TMP/chamadas.txt" ] || falhar "o passo não chamou o 'claude' nenhuma vez:
$saida"
    grep -q "plugin marketplace add" "$TMP/chamadas.txt" \
      || falhar "o passo não registrou o marketplace"
    grep -q "plugin install mustard@" "$TMP/chamadas.txt" \
      || grep -q "plugin update mustard@" "$TMP/chamadas.txt" \
      || falhar "o passo não instalou nem atualizou o plugin"

    echo "ok: com o 'claude' no PATH, o passo registra o marketplace e instala o plugin"
    ;;

  sem-claude)
    # PATH mínimo: só os utilitários que o passo usa, e nenhum `claude`.
    for u in sh id uname grep stat timeout; do
      alvo=$(command -v "$u" 2>/dev/null) || continue
      [ -n "$alvo" ] && ln -sf "$alvo" "$TMP/bin/$u"
    done

    saida=$(PATH="$TMP/bin" HOME="$TMP/home" "$SH" "$PASSO" 2>&1)
    status=$?

    [ "$status" -eq 0 ] || falhar "sem o 'claude' o passo saiu com $status; tem de sair 0:
$saida"
    echo "$saida" | grep -q "plugin marketplace add" \
      || falhar "o passo não imprimiu as instruções manuais:
$saida"

    echo "ok: sem o 'claude' no PATH, o passo sai 0 e ensina o caminho manual"
    ;;

  liga-o-plugin)
    escrever_claude_falso

    saida=$(rodar_o_passo)
    status=$?

    [ "$status" -eq 0 ] || falhar "o passo saiu com $status, e ele é fail-open:
$saida"
    [ -f "$TMP/chamadas.txt" ] || falhar "o passo não chamou o 'claude' nenhuma vez:
$saida"
    grep -q "plugin enable mustard@" "$TMP/chamadas.txt" \
      || falhar "o passo instalou e parou por aí: nunca chamou 'plugin enable'.
O plugin fica instalado e DESLIGADO — zero hooks, zero comandos /mustard:*.
o que ele chamou:
$(cat "$TMP/chamadas.txt")"

    echo "ok: depois de instalar, o passo LIGA o plugin"
    ;;

  baixa-os-binarios)
    # O plugin FALSO. O passo pergunta ao `claude` onde o plugin foi parar e
    # invoca o `mustard-boot` de lá; este aqui só anota que foi chamado.
    # De propósito SEM `chmod +x`: o passo invoca por `sh`, não pelo bit de
    # execução, e é isso que o faz funcionar num cache extraído sem permissões.
    mkdir -p "$TMP/plugin/bin"
    cat > "$TMP/plugin/bin/mustard-boot" <<EOF
#!/bin/sh
echo "\$@" >> "$TMP/boot.txt"
exit 0
EOF

    escrever_claude_falso "$TMP/plugin"

    saida=$(rodar_o_passo)
    status=$?

    [ "$status" -eq 0 ] || falhar "o passo saiu com $status, e ele é fail-open:
$saida"
    [ -f "$TMP/boot.txt" ] || falhar "o passo nunca disparou o mustard-boot.
Os binários do plugin só desceriam por hook — e hook não roda com o plugin
recém-instalado, que é a armadilha inteira:
$saida"
    grep -q -- "--version" "$TMP/boot.txt" \
      || falhar "o mustard-boot foi chamado sem argumento nenhum; sem '--version'
o binário sai com erro de uso e o passo acusaria falha onde não houve"

    echo "ok: o passo dispara a descida dos binários ali mesmo, sem hook nenhum"
    ;;

  nao-trava-o-instalador)
    # Um `mustard-boot` que NUNCA volta. Até esta unidade ele só rodava dentro
    # de um hook, onde o Claude Code corta em 120s. Agora ele roda no caminho
    # crítico do instalador, segurando o cadeado do apt — e o `curl` lá dentro
    # não tem prazo próprio. Um `apt install` que espera para sempre é pior que
    # um plugin desatualizado.
    command -v timeout >/dev/null 2>&1 \
      || { echo "pulado: não há 'timeout' nesta máquina, e o prazo depende dele"; exit 0; }

    mkdir -p "$TMP/plugin/bin"
    cat > "$TMP/plugin/bin/mustard-boot" <<'EOF'
#!/bin/sh
sleep 60
EOF

    escrever_claude_falso "$TMP/plugin"

    LIMITE=2
    inicio=$(date +%s)
    saida=$(rodar_o_passo)
    status=$?
    gasto=$(( $(date +%s) - inicio ))

    [ "$status" -eq 0 ] || falhar "o passo saiu com $status, e ele é fail-open:
$saida"
    [ "$gasto" -lt 30 ] || falhar "o passo esperou ${gasto}s por um mustard-boot que
nunca volta. Com o prazo valendo ele tinha de desistir em 2s — do jeito que
está, um portal cativo congela o 'apt install' para sempre"
    echo "$saida" | grep -q "passou de 2" \
      || falhar "o passo desistiu, mas não disse que foi por prazo — quem lê vai
procurar um erro de rede que não existe:
$saida"

    echo "ok: um mustard-boot travado não segura o instalador (largou em ${gasto}s)"
    ;;

  paridade-ps1)
    [ -f "$GEMEO" ] || falhar "não achei o gêmeo Windows em $GEMEO"

    faltou=0

    # O corpo do arquivo SEM os comentários. As duas linguagens comentam com `#`
    # no começo da linha, e a primeira versão deste teste passava porque achava
    # `exit 0` dentro de um comentário do cabeçalho do .ps1. Um teste que lê
    # comentário não está lendo código.
    sem_comentarios() { grep -v '^[[:space:]]*#' "$1"; }

    # Quantas linhas de código casam com o texto. A contagem importa: uma função
    # aparece uma vez ao ser DEFINIDA e outra ao ser CHAMADA, e foi exatamente
    # apagando a chamada — deixando a definição — que o gêmeo Windows conseguiu
    # instalar e parar sem este teste reclamar.
    contar() { sem_comentarios "$1" | grep -cF -- "$2"; }

    exigir() {
      rotulo="$1"; no_sh="$2"; min_sh="$3"; no_ps1="$4"; min_ps1="$5"
      n=$(contar "$PASSO" "$no_sh")
      [ "$n" -ge "$min_sh" ] \
        || { echo "  plugin-step.sh:  $rotulo — achei $n, esperava $min_sh" >&2; faltou=1; }
      n=$(contar "$GEMEO" "$no_ps1")
      [ "$n" -ge "$min_ps1" ] \
        || { echo "  plugin-step.ps1: $rotulo — achei $n, esperava $min_ps1" >&2; faltou=1; }
    }

    exigir "registrar o marketplace"    "plugin marketplace add"   1 "plugin marketplace add"      1
    exigir "instalar ou atualizar"      'claude plugin "$acao"'    1 'claude plugin $acao'         1
    exigir "o comando que LIGA"         "plugin enable"            1 "plugin enable"               1
    exigir "e a CHAMADA que liga"       "ligar_o_plugin"           2 "Enable-Plugin"               2
    exigir "onde o plugin ficou"        "plugin list --json"       1 "plugin list --json"          1
    exigir "o comando que BAIXA"        "mustard-boot"             1 "mustard-boot"                1
    exigir "e a CHAMADA que baixa"      "baixar_os_binarios"       2 "Start-BinaryDownload"        2
    exigir "o prazo da descida"         "MUSTARD_PLUGIN_STEP_TIMEOUT" 1 "MUSTARD_PLUGIN_STEP_TIMEOUT" 1
    exigir "as instruções manuais"      "instrucoes_manuais"       2 "Show-ManualSteps"            2
    exigir "sair com 0 (fail-open)"     "exit 0"                   1 "exit 0"                      1

    # Os dois nomes que decidem QUAL plugin é instalado. Uma divergência aqui
    # não quebra nenhum dos dois arquivos sozinho: instala o plugin errado em um
    # só dos sistemas, que é bem pior de achar.
    for chave in "rubensrpj/mustard" "mustard-local"; do
      grep -qF -- "$chave" "$PASSO" \
        || { echo "  plugin-step.sh:  falta a constante $chave" >&2; faltou=1; }
      grep -qF -- "$chave" "$GEMEO" \
        || { echo "  plugin-step.ps1: falta a constante $chave" >&2; faltou=1; }
    done

    [ "$faltou" -eq 0 ] || falhar "os gêmeos divergiram — veja as linhas acima"

    # Comparar não é executar, e vale dizer isso em voz alta: nada neste
    # repositório RODA o plugin-step.ps1. Onde houver PowerShell, ao menos o
    # parser dele opina.
    pwsh_bin=$(command -v pwsh 2>/dev/null || command -v powershell 2>/dev/null || true)
    if [ -n "$pwsh_bin" ]; then
      "$pwsh_bin" -NoProfile -Command "
        \$erros = \$null
        [System.Management.Automation.Language.Parser]::ParseFile('$GEMEO', [ref]\$null, [ref]\$erros) > \$null
        if (\$erros -and \$erros.Count) { \$erros | ForEach-Object { \$_.Message }; exit 1 }
      " || falhar "o plugin-step.ps1 não passa nem no parser do PowerShell"
      echo "ok: plugin-step.sh e plugin-step.ps1 dão os mesmos passos (e o .ps1 parseia)"
    else
      echo "ok: plugin-step.sh e plugin-step.ps1 dão os mesmos passos"
      echo "    (sem PowerShell aqui: o gêmeo foi comparado, não executado)"
    fi
    ;;

  *)
    echo "uso: $0 com-claude|sem-claude|liga-o-plugin|baixa-os-binarios|nao-trava-o-instalador|paridade-ps1" >&2
    exit 1
    ;;
esac
