#!/usr/bin/env sh
# ============================================================================
# plugin-step.test.sh — exercita `packaging/installer/plugin-step.sh` nos dois
# caminhos que decidem se o instalador cumpre o que promete.
#
#   com-claude — há um `claude` no PATH: o passo TEM de registrar o marketplace
#                e instalar-ou-atualizar o plugin. É o caminho normal.
#   sem-claude — não há `claude` nenhum: o passo TEM de sair com 0 e ensinar o
#                caminho manual. Um instalador que morre aqui faria a pessoa
#                concluir que o pacote não foi instalado — e ele foi.
#
# O `claude` do caso feliz é FALSO: um script que só anota o que lhe pediram. O
# teste não pode instalar plugin de verdade na máquina de quem o roda.
#
# Uso: sh packaging/installer/tests/plugin-step.test.sh com-claude|sem-claude
# ============================================================================
set -u

CASO="${1:-}"
AQUI=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) || exit 1
PASSO="$AQUI/../plugin-step.sh"

# Resolvido AGORA, com o PATH ainda inteiro: os casos abaixo estreitam o PATH do
# processo filho, e um `sh` procurado depois disso poderia não ser achado.
SH=$(command -v sh) || { echo "FALHA: não achei um 'sh'" >&2; exit 1; }

[ -f "$PASSO" ] || { echo "FALHA: não achei $PASSO" >&2; exit 1; }

TMP=$(mktemp -d) || exit 1
trap 'rm -rf "$TMP"' EXIT
trap 'rm -rf "$TMP"; exit 130' INT TERM HUP

mkdir -p "$TMP/bin" "$TMP/home"

falhar() { echo "FALHA: $1" >&2; exit 1; }

case "$CASO" in
  com-claude)
    # O `claude` falso anota cada invocação numa linha e sai bem-sucedido.
    # `plugin list` responde vazio, então o passo deve concluir que precisa
    # INSTALAR — e o teste aceita install OU update, porque qual dos dois é
    # decisão do passo, não deste teste.
    cat > "$TMP/bin/claude" <<EOF
#!/bin/sh
echo "\$@" >> "$TMP/chamadas.txt"
exit 0
EOF
    chmod +x "$TMP/bin/claude"

    saida=$(PATH="$TMP/bin:$PATH" HOME="$TMP/home" "$SH" "$PASSO" 2>&1)
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

  *)
    echo "uso: $0 com-claude|sem-claude" >&2
    exit 1
    ;;
esac
