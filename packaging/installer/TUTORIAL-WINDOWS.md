# Mustard no Windows — tutorial de instalação completa

Este tutorial explica, passo a passo, como instalar o Mustard **completo** no
Windows 10/11: os comandos de linha (`mustard`, `mustard-rt`, `mustard-mcp`,
`scan`, `rtk`) **e** o **Mustard Dashboard** (aplicativo desktop). Tudo num
único instalador `.exe` — você não precisa instalar Rust, Node ou qualquer
ferramenta de desenvolvimento.

O arquivo a baixar é:

```
Mustard Dashboard_<versao>_x64-setup.exe
```

O que o instalador faz:

```
- instala o Mustard Dashboard (app) na pasta do programa
- instala junto os binários do CLI (mustard, mustard-rt, mustard-mcp, scan, rtk)
  e os templates do `mustard init`
- adiciona o CLI ao PATH do seu usuário
- cria o atalho "Mustard Dashboard" no Menu Iniciar
```

O que ele **não** faz: instalar o plugin do Claude Code. Esse é o item 6 deste
tutorial, e sem ele o Mustard não tem comandos nem hooks dentro do Claude.

---

## 1. Pré-requisitos

| Requisito | Como verificar |
|---|---|
| Windows 10 ou 11 | `winver` (caixa Executar) |
| Claude Code instalado e logado (o Mustard trabalha dentro dele) | `claude --version` |

Se ainda não tiver o Claude Code, instale seguindo
<https://docs.claude.com/claude-code> e faça login uma vez com `claude`.

---

## 2. Baixar

Baixe o arquivo **`Mustard Dashboard_<versao>_x64-setup.exe`** da página de
releases (seção **Assets**).

---

## 3. Instalar

1. Dê **duplo-clique** no `...-setup.exe`.
2. **Aviso do SmartScreen** (esperado — o instalador não é assinado): clique em
   **"Mais informações"** e depois em **"Executar assim mesmo"**.
3. Siga o assistente (Avançar → Instalar).
4. **Abra um terminal NOVO** (PowerShell ou Prompt de Comando). O CLI só aparece
   no PATH em terminais abertos **depois** da instalação.

---

## 4. Verificar

Num terminal novo:

```powershell
mustard --version
mustard-rt --version
rtk --version
```

Os três devem responder com a versão. E o **dashboard**: procure
**"Mustard Dashboard"** no **Menu Iniciar**.

---

## 5. Preparar um projeto

Em qualquer projeto que você queira testar:

```powershell
cd C:\caminho\do\seu\projeto
mustard init
```

Isso escreve a pasta `.claude/` (a configuração do projeto) e o `mustard.json` na
raiz. Só isso: os **hooks** do Mustard **não** vêm daqui — o
`.claude/settings.json` que o `init` grava não tem nenhum. Eles chegam junto com
o plugin, que é o passo do item 6, e é por isso que ele não é opcional.

---

## 6. Instalar o plugin dentro do Claude Code

O `.exe` traz **binários e templates**; ele não toca no seu `%USERPROFILE%\.claude`.
Os comandos `/mustard:*`, os agentes e o servidor MCP de memória vêm do **plugin
do Claude Code** — e esse passo é dado **dentro** do Claude Code, não no terminal.

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

**`mustard` não é reconhecido como comando**
O PATH só atualiza em terminais abertos **depois** de instalar. Feche e abra um
terminal novo. Se persistir, faça logoff/login no Windows.

**O SmartScreen não deixa executar**
Clique em **"Mais informações" → "Executar assim mesmo"**. Isso ocorre porque o
instalador ainda não é assinado por um certificado de código.

**O `rtk` não foi encontrado**
Em casos raros o `rtk` não vem no pacote. Instale-o com:
`cargo install --git https://github.com/rtk-ai/rtk` (precisa do Rust) ou
`scoop install rtk`.

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

**O `mustard --version` responde uma versão ANTIGA depois de atualizar**
Sinal de que o terminal está alcançando uma instalação anterior. O número é
gravado dentro do executável quando ele é compilado, então quem responde a versão
velha É o binário velho. Confira qual está sendo achado com
`(Get-Command mustard).Source` e remova do seu PATH de usuário a pasta antiga
(tipicamente `%USERPROFILE%\.mustard\bin`, de uma instalação por `install.ps1`).

---

## 8. Desinstalar

Vá em **Configurações → Aplicativos → Aplicativos instalados**, procure
**"Mustard Dashboard"** e clique em **Desinstalar**. Isso remove o app e o CLI.

Em projetos testados, a pasta `.claude/` e o `mustard.json` podem ser apagados à
vontade.
