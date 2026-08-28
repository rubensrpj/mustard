#!/usr/bin/env python3
"""Todo lint desligado na tabela do workspace carrega a razão escrita ao lado.

POR QUE ESTA TRAVA EXISTE

O repositório chegou a 347 avisos de clippy. Nenhum quebrava nada, e era esse o
problema: com 347 de fundo, um aviso NOVO vira 348 e ninguém o vê. A saída foi
duas: limpar, e passar a REPROVAR o build com qualquer aviso.

Mas "reprovar com qualquer aviso" tem uma válvula de escape óbvia — desligar o
lint. Um `allow` mudo devolve exatamente a cegueira que a limpeza removeu, só
que numa linha em vez de em trezentas. Então o `allow` continua permitido, e
continua sendo a resposta certa em vários casos: o que não é permitido é
desligar sem dizer por quê.

O QUE ELA MEDE

Cada linha `<lint> = "allow"` dentro de `[workspace.lints.clippy]` precisa de
uma razão em prosa: um comentário `#` numa das linhas imediatamente acima (um
bloco de comentário cobre todas as linhas de `allow` que vêm logo abaixo dele),
ou um comentário no fim da própria linha.

O QUE ELA NÃO MEDE — pendência declarada

Os `#[allow(clippy::...)]` espalhados pelo código-fonte. São 59 hoje, quase
todos anteriores a esta unidade e sem razão escrita. Cobri-los aqui reprovaria
o build por trabalho que ninguém pediu, então ficam declarados em vez de
silenciosamente incluídos. Fecha-los é unidade própria.

Sai com 0 quando toda linha tem razão, e com 1 listando as que não têm.
"""

import re
import sys
from pathlib import Path

TABLE = "[workspace.lints.clippy]"
ALLOW = re.compile(r'^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*"allow"\s*(#.*)?$')
SECTION = re.compile(r"^\s*\[")


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    manifest = root / "Cargo.toml"
    if not manifest.is_file():
        print(f"nao achei {manifest}", file=sys.stderr)
        return 1

    lines = manifest.read_text(encoding="utf-8").splitlines()
    try:
        start = next(i for i, l in enumerate(lines) if l.strip() == TABLE) + 1
    except StopIteration:
        print(f"{TABLE} nao existe no Cargo.toml", file=sys.stderr)
        return 1

    naked = []
    # `prose` acumula o bloco de comentário corrente: um `#` reinicia/estende o
    # bloco, uma linha em branco o encerra. É assim que um comentário escrito
    # uma vez cobre o grupo de `allow` logo abaixo dele.
    prose = False
    for i in range(start, len(lines)):
        line = lines[i]
        if SECTION.match(line):
            break
        stripped = line.strip()
        if not stripped or stripped == "#":
            prose = False
            continue
        if stripped.startswith("#"):
            prose = True
            continue
        m = ALLOW.match(line)
        if m:
            if not prose and not m.group(2):
                naked.append((i + 1, m.group(1)))
            continue
        # qualquer outra linha (um `warn`, um `deny`) encerra o bloco de prosa
        prose = False

    if naked:
        print("lints desligados sem razao escrita, em Cargo.toml:", file=sys.stderr)
        for lineno, lint in naked:
            print(f"  linha {lineno}: {lint}", file=sys.stderr)
        print(
            "\nEscreva por que ele nao vale a pena — um comentario `#` acima da "
            "linha, ou no fim dela. Um `allow` mudo devolve a cegueira que os "
            "347 avisos causavam, so que numa linha.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
