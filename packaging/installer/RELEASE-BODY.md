## Instalação — **um** instalador por sistema

| Sistema | O que baixar | Passo a passo |
|---|---|---|
| 🪟 **Windows** 10/11 | **`Mustard Dashboard_{{VERSION}}_x64-setup.exe`** | `TUTORIAL-WINDOWS.md` |
| 🍎 **macOS** 11+ (Intel + Apple Silicon) | **`Mustard-{{VERSION}}-universal.pkg`** | `TUTORIAL-MACOS.md` |
| 🐧 **Linux** (Ubuntu 22.04+) | nada — instale com o `curl` do resumo abaixo (a rota manual usa **`mustard_{{VERSION}}_amd64.deb`** + `install.sh`) | `TUTORIAL-LINUX.md` |

Cada instalador traz **tudo**: o CLI (`mustard`, `mustard-rt`, `mustard-mcp`, `scan`, `rtk`) **e** o **Mustard Dashboard**. Não precisa instalar Rust nem Node.

### Resumo rápido
- **Windows:** execute o `.exe` → no aviso do SmartScreen, *Mais informações* → *Executar assim mesmo* → abra um terminal **novo**.
- **macOS:** abra o `.pkg` (não assinado → **clique com o botão direito → Abrir**) → siga o assistente → abra um terminal **novo**.
- **Linux (Ubuntu 22.04+):** uma linha, sem baixar nada à mão — esta instala **exatamente a v{{VERSION}}**, a versão desta página. Quem fixa é o `MUSTARD_VERSION`: a URL escolhe só qual `install.sh` roda, e o script sozinho sempre buscaria o último Release.
  ```sh
  curl -fsSL https://github.com/rubensrpj/mustard/releases/download/v{{VERSION}}/install.sh | MUSTARD_VERSION={{VERSION}} sh
  ```
  Quer sempre o **último** Release, seja ele qual for? Então é esta, sem fixar versão:
  ```sh
  curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh | sh
  ```
  Quem preferir conferir o `sha256` antes: baixe o `.deb` e o `install.sh` dos *Assets* na mesma pasta e rode `chmod +x install.sh && ./install.sh` — os assets chegam sem a permissão de execução (`TUTORIAL-LINUX.md` detalha as duas rotas).

Depois, em qualquer projeto: **`mustard init`** — e, **dentro do Claude Code**, o plugin (é ele que traz os comandos `/mustard:*`, os hooks e o MCP de memória):

```
/plugin marketplace add rubensrpj/mustard
/plugin install mustard@mustard-local
```

> ⚠️ Os instaladores **não são assinados** — Windows (SmartScreen) e macOS (Gatekeeper) pedem uma confirmação na primeira execução; é esperado. O **passo a passo completo de cada sistema** está nos **Assets** abaixo (`TUTORIAL-WINDOWS.md`, `TUTORIAL-MACOS.md`, `TUTORIAL-LINUX.md`).

---
