# Mustard no Ubuntu — tutorial de instalação completa

Este tutorial explica, passo a passo, como instalar o Mustard **completo** num
Ubuntu: os comandos de linha (`mustard`, `mustard-rt`, `mustard-mcp`, `scan`,
`rtk`) **e** o **Mustard Dashboard** (aplicativo desktop). Tudo num único pacote
`.deb`, instalado com `apt` — você não precisa instalar Rust, Node ou qualquer
ferramenta de desenvolvimento.

O que será instalado (gerenciado pelo apt):

```
/usr/lib/mustard/bin/        binários reais (CLI + dashboard)
/usr/lib/mustard/templates/  a carga que o `mustard init` copia para os projetos
/usr/bin/mustard, …          atalhos no PATH (mustard, mustard-rt, …, mustard-dashboard)
menu de aplicativos           atalho "Mustard Dashboard"
```

---

## 1. Pré-requisitos

| Requisito | Como verificar |
|---|---|
| Ubuntu 22.04 ou mais novo (glibc 2.35+) | `ldd --version` — a 1ª linha mostra a versão |
| Claude Code instalado e logado (o Mustard trabalha dentro dele) | `claude --version` |
| `sudo` (para o `apt install`) | `sudo -v` |

> Por que Ubuntu 22.04+: o dashboard depende do `webkit2gtk-4.1`, que não existe
> no Ubuntu 20.04. O `apt` instala essa dependência automaticamente.

Se ainda não tiver o Claude Code, instale com:

```sh
curl -fsSL https://claude.ai/install.sh | bash
```

e faça login uma vez com `claude` (guia completo em <https://docs.claude.com/claude-code>).

---

## 2. Baixar e descompactar o pacote

Copie o pacote para qualquer pasta (por exemplo, `~/Downloads`) e descompacte
(se ele veio num `.tar.gz` ou `.zip`); ou simplesmente coloque o `install.sh` e o
`mustard_*_amd64.deb` na mesma pasta:

```sh
cd ~/Downloads
# (se vier compactado) tar -xzf mustard-linux.tar.gz && cd mustard-linux
ls
# deve listar: install.sh   mustard_<versao>_amd64.deb   README.txt   TUTORIAL-LINUX.md
```

---

## 3. Instalar (tudo de uma vez)

**a) Instalar tudo:**

```sh
./install.sh
```

**b) Instalar e já preparar um projeto seu para testar** (roda o `mustard init`
no projeto indicado):

```sh
./install.sh /caminho/do/seu/projeto
```

O instalador chama o `apt`, que:

1. instala os binários do CLI em `/usr/lib/mustard/bin` e os templates em
   `/usr/lib/mustard/templates`, criando os atalhos em `/usr/bin`;
2. instala o **Mustard Dashboard** e **resolve sozinho** as dependências de
   sistema dele (`webkit2gtk-4.1`, `gtk`, …);
3. adiciona o atalho "Mustard Dashboard" ao menu de aplicativos;
4. se você passou um projeto, roda `mustard init` nele (cria a pasta `.claude/`
   e o `mustard.json`).

> Prefere o comando do apt direto? É só:
> `sudo apt install ./mustard_<versao>_amd64.deb`

---

## 4. Verificar

```sh
mustard --version
mustard-rt --version
rtk --version
```

Os três devem responder com a versão. E o **dashboard**: procure
**"Mustard Dashboard"** no menu de aplicativos, ou rode no terminal:

```sh
mustard-dashboard
```

---

## 5. Preparar um projeto (se ainda não preparou)

Em qualquer projeto que você queira testar:

```sh
cd /caminho/do/seu/projeto
mustard init
```

Isso cria a pasta `.claude/` (hooks, skills e configuração) e o
`mustard.json` na raiz. A partir daí é só **abrir o Claude Code normalmente
dentro do projeto** — os hooks do Mustard já estão ligados via
`.claude/settings.json`; nenhum passo extra é necessário.

Comandos úteis dentro do Claude Code: `/scan` (mapeia o projeto),
`/feature` (pipeline de feature), `/bugfix`, `/status`.

---

## 6. Problemas comuns

**`mustard: command not found` logo após instalar**
O `/usr/bin` já está no PATH de qualquer shell, então isso é raro. Se acontecer,
abra um novo terminal. Confirme a instalação com `dpkg -l mustard`.

**O dashboard não abre / erro de biblioteca `webkit`**
O `apt` deveria ter resolvido. Force a correção de dependências:

```sh
sudo apt --fix-broken install
```

**`apt` reclama que o pacote é de terceiro / não confiável**
É um `.deb` local (não vem de um repositório assinado) — isso é esperado. O
`apt install ./arquivo.deb` instala mesmo assim.

**Versão antiga do Ubuntu (20.04 ou anterior)**
O dashboard exige glibc 2.35+ (Ubuntu 22.04+). Atualize a distro para usar o
pacote completo.

---

## 7. Desinstalar

Como é um pacote do apt, remover é uma linha:

```sh
sudo apt remove mustard
```

Em projetos testados, a pasta `.claude/` e o `mustard.json` podem ser apagados à
vontade.
