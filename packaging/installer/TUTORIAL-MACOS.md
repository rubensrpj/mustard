# Mustard no macOS — tutorial de instalação completa

Este tutorial explica, passo a passo, como instalar o Mustard **completo** no
macOS: os comandos de linha (`mustard`, `mustard-rt`, `mustard-mcp`, `scan`,
`rtk`) **e** o **Mustard Dashboard** (aplicativo desktop). Tudo num único
instalador `.pkg` — você não precisa instalar Rust, Node ou qualquer ferramenta
de desenvolvimento.

O arquivo a baixar é:

```
Mustard-<versao>-universal.pkg
```

É **universal**: roda tanto em Macs Apple Silicon (M1/M2/M3…) quanto Intel.

O que o instalador faz:

```
- instala o "Mustard Dashboard.app" em /Applications (com o CLI e os
  templates embutidos)
- cria os atalhos do CLI no PATH, em /usr/local/bin
  (mustard, mustard-rt, mustard-mcp, scan, rtk)
```

O que ele **não** faz: instalar o plugin do Claude Code. Esse é o item 6 deste
tutorial, e sem ele o Mustard não tem comandos nem hooks dentro do Claude.

---

## 1. Pré-requisitos

| Requisito | Como verificar |
|---|---|
| macOS 11 (Big Sur) ou mais novo | menu  → "Sobre Este Mac" |
| Claude Code instalado e logado (o Mustard trabalha dentro dele) | `claude --version` |

Se ainda não tiver o Claude Code, instale com:

```sh
curl -fsSL https://claude.ai/install.sh | bash
```

e faça login uma vez com `claude` (guia em <https://docs.claude.com/claude-code>).

---

## 2. Baixar

Baixe o arquivo **`Mustard-<versao>-universal.pkg`** da página de releases
(seção **Assets**).

---

## 3. Instalar

1. Dê **duplo-clique** no `Mustard-<versao>-universal.pkg`.
2. **Gatekeeper** pode recusar ("não foi possível verificar o desenvolvedor" —
   esperado, o pacote não é assinado/notarizado). Para liberar:
   - **clique com o botão direito** (ou Control+clique) no `.pkg` → **Abrir** →
     confirme **Abrir**; **ou**
   - vá em **Ajustes do Sistema → Privacidade e Segurança**, role até o aviso do
     Mustard e clique em **"Abrir assim mesmo"**.
3. Siga o assistente (Continuar → Instalar; pede sua senha de administrador).
4. **Abra um terminal NOVO**. O CLI só aparece no PATH em terminais abertos
   **depois** da instalação.

---

## 4. Verificar

Num terminal novo:

```sh
mustard --version
mustard-rt --version
rtk --version
```

Os três devem responder com a versão. E o **dashboard**: abra o **Launchpad**
(ou a pasta **Aplicativos**) e procure **"Mustard Dashboard"**.

---

## 5. Preparar um projeto

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

O `.pkg` traz **binários e templates**; ele não toca no seu `~/.claude`. Os
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

**`mustard: command not found`**
O CLI fica em `/usr/local/bin`, que está no PATH padrão. Abra um terminal novo.
Se você usa um shell incomum, confirme que `/usr/local/bin` está no seu `PATH`.

**"Mustard não pode ser aberto porque o desenvolvedor não pode ser verificado"**
É o Gatekeeper (o pacote não é notarizado). Use **clique-direito → Abrir**, ou
**Ajustes → Privacidade e Segurança → Abrir assim mesmo**. O instalador já
remove a quarentena dos binários durante a instalação.

**O `rtk` não foi encontrado**
Em casos raros o `rtk` não vem no pacote. Instale-o com `brew install rtk` ou
`cargo install --git https://github.com/rtk-ai/rtk` (precisa do Rust).

**Dentro do Claude Code aparece só a barra de status, e nenhum comando `/mustard:*`**
Falta o item 6: o plugin não foi instalado. O `mustard init` semeia a barra de
status em `.claude/settings.json`, então o projeto PARECE instalado mesmo sem o
plugin. Rode os dois comandos do item 6 e recarregue o Claude Code.

**`Plugin "mustard" not found in any marketplace`**
Falta registrar o marketplace: rode `/plugin marketplace add rubensrpj/mustard`
**antes** do `/plugin install mustard@mustard-local` (item 6). Se já tinha
registrado, atualize a cópia local com `/plugin marketplace update mustard-local`
e instale de novo.

**`/plugin marketplace add rubensrpj/mustard` falha com erro de clone/autenticação**
O `add` também aceita a URL completa do repositório, que é a forma a usar quando o
atalho não consegue clonar:
`/plugin marketplace add https://github.com/rubensrpj/mustard.git`.

---

## 8. Desinstalar

```sh
sudo rm -rf "/Applications/Mustard Dashboard.app"
sudo rm -f /usr/local/bin/mustard /usr/local/bin/mustard-rt \
           /usr/local/bin/mustard-mcp /usr/local/bin/scan /usr/local/bin/rtk
```

Em projetos testados, a pasta `.claude/` e o `mustard.json` podem ser apagados à
vontade.
