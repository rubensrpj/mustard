#!/usr/bin/env sh
# ============================================================================
# plugin-step.test.sh — exercita `packaging/installer/plugin-step.sh` nos
# caminhos que decidem se o instalador cumpre o que promete.
#
#   com-claude ......... há um `claude` no PATH: o passo TEM de registrar o
#                        marketplace e instalar-ou-atualizar o plugin. É o
#                        caminho normal.
#   sem-claude ......... não há `claude` nenhum: o passo TEM de sair com 0 e
#                        ensinar o caminho manual. Um instalador que morre aqui
#                        faria a pessoa concluir que o pacote não foi instalado
#                        — e ele foi.
#   liga-o-plugin ...... instalar NÃO basta: `claude plugin install` deixa o
#                        plugin desligado, e plugin desligado é zero hooks e
#                        zero comandos /mustard:*. O passo TEM de chamar
#                        `plugin enable` depois de instalar.
#   baixa-os-binarios .. `plugin/bin/*` são artefatos de build e nunca entram no
#                        git. Quem os baixa é o `mustard-boot` — que é um HOOK,
#                        e hook não roda com o plugin recém-instalado. O passo
#                        TEM de disparar o `mustard-boot` ele mesmo, ali, sem
#                        hook nenhum no meio.
#   paridade-ps1 ....... o gêmeo Windows não pode ficar para trás. Reprova
#                        quando um dos dois arquivos ganha um passo e o outro
#                        não.
#
# Os três primeiros casos e o quarto medem o mesmo ciclo por dentro: foi ele
# que, no campo em 2026-08-28, consumiu três instalações seguidas do .exe e
# uma tarde de diagnóstico à mão. Ligar sem baixar deixa o plugin dormente;
# baixar sem ligar não roda. Por isso cada metade tem o seu caso.
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
rodar_o_passo() {
  PATH="$TMP/bin:$PATH" HOME="$TMP/home" "$SH" "$PASSO" 2>&1
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
    for u in sh id uname grep stat; do
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

  paridade-ps1)
    [ -f "$GEMEO" ] || falhar "não achei o gêmeo Windows em $GEMEO"

    faltou=0

    # Cada linha é um passo que os DOIS arquivos têm de ter, na forma que cada
    # linguagem lhe dá. Um passo que entra só de um lado reprova aqui — foi
    # exatamente assim que o Windows ficou para trás antes.
    exigir() {
      rotulo="$1"; no_sh="$2"; no_ps1="$3"
      grep -qF -- "$no_sh" "$PASSO" \
        || { echo "  falta no plugin-step.sh:  $rotulo" >&2; faltou=1; }
      grep -qF -- "$no_ps1" "$GEMEO" \
        || { echo "  falta no plugin-step.ps1: $rotulo" >&2; faltou=1; }
    }

    exigir "registrar o marketplace"       "plugin marketplace add"    "plugin marketplace add"
    exigir "instalar ou atualizar"         'claude plugin "$acao"'     'claude plugin $acao'
    exigir "LIGAR o plugin"                "plugin enable"             "plugin enable"
    exigir "perguntar onde o plugin ficou" "plugin list --json"        "plugin list --json"
    exigir "BAIXAR os binários"            "mustard-boot"              "mustard-boot"
    exigir "as instruções manuais"         "instrucoes_manuais"        "Show-ManualSteps"
    exigir "sair sempre com 0 (fail-open)" "exit 0"                    "exit 0"

    # Os dois nomes que decidem QUAL plugin é instalado. Uma divergência aqui
    # não quebra nenhum dos dois arquivos sozinho: instala o plugin errado em um
    # só dos sistemas, que é bem pior de achar.
    for chave in "rubensrpj/mustard" "mustard-local"; do
      grep -qF -- "$chave" "$PASSO" \
        || { echo "  falta no plugin-step.sh:  a constante $chave" >&2; faltou=1; }
      grep -qF -- "$chave" "$GEMEO" \
        || { echo "  falta no plugin-step.ps1: a constante $chave" >&2; faltou=1; }
    done

    [ "$faltou" -eq 0 ] || falhar "os gêmeos divergiram — veja as linhas acima"

    echo "ok: plugin-step.sh e plugin-step.ps1 dão os mesmos passos"
    ;;

  *)
    echo "uso: $0 com-claude|sem-claude|liga-o-plugin|baixa-os-binarios|paridade-ps1" >&2
    exit 1
    ;;
esac
