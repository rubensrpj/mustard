# Mustard no Ubuntu — tutorial de instalação completa

Este tutorial explica, passo a passo, como instalar o Mustard **completo** num
Ubuntu: os comandos de linha (`mustard`, `mustard-rt`, `mustard-mcp`, `scan`,
`rtk`) **e** o **Mustard Dashboard**, que é um **servidor**: ele abre uma porta
na sua máquina e você vê o painel no navegador. Tudo num único pacote
`.deb`, instalado com `apt` — você não precisa instalar Rust, Node ou qualquer
ferramenta de desenvolvimento. **Nem baixar o pacote à mão**: a instalação cabe
numa linha (item 2); baixar o `.deb` é a rota alternativa (item 3), para quem
quer conferir o `sha256` antes.

O que será instalado (gerenciado pelo apt):

```
/usr/lib/mustard/bin/        binários reais (CLI + mustard-dashboard)
/usr/lib/mustard/bin/dist/   os arquivos da tela que o servidor serve
/usr/lib/mustard/templates/  a carga que o `mustard init` copia para os projetos
/usr/bin/mustard, …          atalhos no PATH (mustard, mustard-rt, …, mustard-dashboard)
menu de aplicativos           atalho "Mustard Dashboard" (inicia o servidor)
```

---

## 1. Pré-requisitos

| Requisito | Como verificar |
|---|---|
| Ubuntu 22.04 ou mais novo (glibc 2.35+) | `ldd --version` — a 1ª linha mostra a versão |
| Claude Code instalado e logado (o Mustard trabalha dentro dele) | `claude --version` |
| `sudo` (para o `apt install`) | `sudo -v` |

> Por que Ubuntu 22.04+: é a glibc contra a qual os binários são compilados. Não
> há mais nenhuma biblioteca gráfica na conta — o dashboard virou um servidor
> HTTP e a tela quem desenha é o seu navegador.

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
curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh | MUSTARD_VERSION=0.1.35 sh
```

> O `0.1.35` acima é um exemplo: troque pelo número que a página de
> [Releases](https://github.com/rubensrpj/mustard/releases) mostrar. Escreva o
> número **literal** — um `<versao>` no lugar dele não é um espaço para
> preencher: o `<` é redirecionamento de entrada, então o shell deixaria a
> variável vazia e responderia `sh: versao: No such file or directory`.

O instalador chama o `apt`, que:

1. instala os binários do CLI em `/usr/lib/mustard/bin` e os templates em
   `/usr/lib/mustard/templates`, criando os atalhos em `/usr/bin`;
2. instala o **mustard-dashboard** (o servidor) com os arquivos da tela ao lado
   dele;
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

Os três devem responder com a versão.

E o **dashboard**: rode no terminal, de dentro da pasta onde ficam seus
projetos — a varredura começa no diretório de onde o servidor foi iniciado:

```sh
cd ~/code
mustard-dashboard
```

Ele imprime onde está servindo e abre o navegador sozinho quando há sessão
gráfica:

```
mustard-dashboard: serving /home/voce/code at http://127.0.0.1:7777/
```

Sem sessão gráfica (por SSH, num contêiner) ele **não** morre: imprime a URL e
segue servindo. Ctrl+C para. O atalho **"Mustard Dashboard"** no menu de
aplicativos faz o mesmo, num terminal, a partir da sua pasta pessoal.

Opções úteis:

| Opção | Para quê |
|---|---|
| `--root /outra/pasta` | varre outra pasta em vez do diretório atual |
| `--port 8080` | outra porta (ou a variável `MUSTARD_DASHBOARD_PORT`). Porta ocupada não é erro: ele usa a próxima livre e imprime qual |
| `--host 0.0.0.0` | **expõe na rede** — só assim outra máquina alcança o painel |
| `--no-open` | não abre o navegador |

> ⚠️ Sem `--host`, o painel só responde na própria máquina (`127.0.0.1`). Isso é
> proposital: ele lê o `.claude/` de **todos** os seus projetos, então expor à
> rede tem de ser um ato, não um esquecimento. Para alcançar de outro
> computador (por exemplo por Tailscale), rode
> `mustard-dashboard --host 0.0.0.0` e acesse `http://<ip-da-maquina>:7777/`.

---

## 5. Preparar um projeto (se ainda não preparou)

Em qualquer projeto que você queira testar:

```sh
cd /caminho/do/seu/projeto
mustard init
```

Isso escreve a pasta `.claude/` (a configuração do projeto) e o `mustard.json` na
raiz. Só isso: os **hooks** do Mustard **não** vêm daqui — o
`.claude/settings.json` que o `init` grava não tem nenhum. Eles chegam junto com
o plugin, que é o passo do item 6, e é por isso que ele não é opcional.

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

**O navegador abre e a página fica em branco / "dashboard assets not found"**
Falta a pasta da tela ao lado do binário. Confira que ela veio no pacote:

```sh
ls /usr/lib/mustard/bin/dist/index.html
```

Se não existir, reinstale o `.deb`. Para apontar outra cópia dos arquivos, use a
variável `MUSTARD_DASHBOARD_DIST`.

**Nada abre e o terminal diz `no graphical session`**
Não é erro: por SSH ou em contêiner não há navegador para abrir. O servidor está
de pé — abra a URL que ele imprimiu. Para alcançá-lo de fora da máquina, veja o
`--host` do item 4.

**`apt` reclama que o pacote é de terceiro / não confiável**
É um `.deb` local (não vem de um repositório assinado) — isso é esperado. O
`apt install ./arquivo.deb` instala mesmo assim.

**Versão antiga do Ubuntu (20.04 ou anterior)**
Os binários exigem glibc 2.35+ (Ubuntu 22.04+). Atualize a distro para usar o
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
