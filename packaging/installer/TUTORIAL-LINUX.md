# Mustard no Ubuntu — tutorial de instalação completa

Este tutorial explica, passo a passo, como instalar o Mustard **completo** num
Ubuntu: os comandos de linha (`mustard`, `mustard-rt`, `mustard-mcp`, `scan`,
`rtk`) **e** o **Mustard Dashboard** (aplicativo desktop). Tudo num único pacote
`.deb`, instalado com `apt` — você não precisa instalar Rust, Node ou qualquer
ferramenta de desenvolvimento. **Nem baixar o pacote à mão**: a instalação cabe
numa linha (item 2); baixar o `.deb` é a rota alternativa (item 3), para quem
quer conferir o `sha256` antes.

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

## 2. Instalar numa linha (rota recomendada)

Cole **esta linha** num terminal. Ela baixa o instalador, que por sua vez baixa o
`.deb` do último Release e entrega ao `apt` — você não baixa arquivo nenhum à mão:

```sh
curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh | sh
```

**Instalar e já preparar um projeto seu para testar** (roda o `mustard init` no
projeto indicado) — o caminho vai depois do `-s --`:

```sh
curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh | sh -s -- /caminho/do/seu/projeto
```

Duas variações úteis:

```sh
# só mostra o que seria feito; não instala nada
curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh | sh -s -- --dry-run

# fixa uma versão em vez de pegar o último Release
# (troque <versao> pelo número que a página do Release mostra, ex.: 0.1.35)
curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh | MUSTARD_VERSION=<versao> sh
```

O instalador chama o `apt`, que:

1. instala os binários do CLI em `/usr/lib/mustard/bin` e os templates em
   `/usr/lib/mustard/templates`, criando os atalhos em `/usr/bin`;
2. instala o **Mustard Dashboard** e **resolve sozinho** as dependências de
   sistema dele (`webkit2gtk-4.1`, `gtk`, …);
3. adiciona o atalho "Mustard Dashboard" ao menu de aplicativos;
4. se você passou um projeto, roda `mustard init` nele (cria a pasta `.claude/`
   e o `mustard.json`).

---

## 3. Alternativa: baixar o `.deb` à mão (permite conferir o sha256)

Prefere inspecionar o pacote antes de instalar? Baixe da página de
[Releases](https://github.com/rubensrpj/mustard/releases) (seção **Assets**) os
dois arquivos — `install.sh` e `mustard_<versao>_amd64.deb` — para a mesma pasta:

```sh
cd ~/Downloads
ls
# deve listar: install.sh   mustard_<versao>_amd64.deb   (e, se veio no pacote, README.txt e TUTORIAL-LINUX.md)
```

Confira o resumo **sha256** do `.deb` e compare com o `digest` que a página do
Release mostra para esse mesmo asset:

```sh
sha256sum mustard_<versao>_amd64.deb
```

Batendo, instale. Com um `.deb` ao lado dele, o `install.sh` usa **esse arquivo**
e não baixa nada:

```sh
chmod +x install.sh
./install.sh                        # instala tudo
./install.sh /caminho/do/projeto    # instala e roda `mustard init` no projeto
```

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
`mustard.json` na raiz — os hooks do Mustard já ficam ligados via
`.claude/settings.json`. Falta **um** passo, o do item 6.

---

## 6. Instalar o plugin dentro do Claude Code

O `.deb` traz **binários e templates**; ele não toca no seu `~/.claude`. Os
comandos `/mustard:*`, os agentes e o servidor MCP de memória vêm do **plugin do
Claude Code** — e esse passo é dado **dentro** do Claude Code, não no terminal.

Abra o Claude Code no projeto (`claude`) e digite:

```
/plugin marketplace add rubensrpj/mustard
/plugin install mustard@mustard-local
```

O primeiro comando registra o *marketplace* (o repositório do Mustard, que traz o
`.claude-plugin/marketplace.json`); o segundo instala o plugin `mustard` a partir
dele — daí o `@mustard-local`, que é o **nome do marketplace**, não um caminho.
Recarregue o Claude Code (feche e abra) para os hooks e comandos entrarem.

São quatro portas dentro do Claude Code: `/mustard:git`, `/mustard:pr`,
`/mustard:spec` e `/mustard:upsert`. Para COMEÇAR um trabalho não há comando —
descreva o pedido em palavras suas e o roteador escolhe o fluxo sozinho.

---

## 7. Problemas comuns

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

**`Plugin "mustard" not found in any marketplace`**
Falta registrar o marketplace: rode `/plugin marketplace add rubensrpj/mustard`
**antes** do `/plugin install mustard@mustard-local` (item 6). Se já tinha
registrado, atualize a cópia local com `/plugin marketplace update mustard-local`
e instale de novo.

**`/plugin marketplace add rubensrpj/mustard` falha com erro de clone/autenticação**
O `add` também aceita a URL completa do repositório, que é a forma a usar quando o
atalho não consegue clonar:
`/plugin marketplace add https://github.com/rubensrpj/mustard.git`.

**O `curl … | sh` não instala nada / "não achei o pacote"**
Sem rede, o instalador não consegue resolver a última versão. Confira a conexão,
ou siga a rota manual do item 3 (baixando o `.deb` da página de Releases).

---

## 8. Desinstalar

Como é um pacote do apt, remover é uma linha:

```sh
sudo apt remove mustard
```

Em projetos testados, a pasta `.claude/` e o `mustard.json` podem ser apagados à
vontade.
