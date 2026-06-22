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

Isso cria a pasta `.claude/` (hooks, skills e configuração) e o `mustard.json`
na raiz. A partir daí é só **abrir o Claude Code normalmente dentro do
projeto** — os hooks do Mustard já estão ligados via `.claude/settings.json`.

Comandos úteis dentro do Claude Code: `/scan` (mapeia o projeto),
`/feature` (pipeline de feature), `/bugfix`, `/status`.

---

## 6. Problemas comuns

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

---

## 7. Desinstalar

Vá em **Configurações → Aplicativos → Aplicativos instalados**, procure
**"Mustard Dashboard"** e clique em **Desinstalar**. Isso remove o app e o CLI.

Em projetos testados, a pasta `.claude/` e o `mustard.json` podem ser apagados à
vontade.
