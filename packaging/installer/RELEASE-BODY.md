## Instalação — baixe **um** arquivo conforme o seu sistema

| Sistema | Baixe este arquivo | Passo a passo |
|---|---|---|
| 🪟 **Windows** 10/11 | **`Mustard Dashboard_{{VERSION}}_x64-setup.exe`** | `TUTORIAL-WINDOWS.md` |
| 🍎 **macOS** 11+ (Intel + Apple Silicon) | **`Mustard-{{VERSION}}-universal.pkg`** | `TUTORIAL-MACOS.md` |
| 🐧 **Linux** (Ubuntu 22.04+) | **`mustard_{{VERSION}}_amd64.deb`** + `install.sh` | `TUTORIAL-LINUX.md` |

Cada instalador traz **tudo**: o CLI (`mustard`, `mustard-rt`, `mustard-mcp`, `scan`, `rtk`) **e** o **Mustard Dashboard**. Não precisa instalar Rust nem Node.

### Resumo rápido
- **Windows:** execute o `.exe` → no aviso do SmartScreen, *Mais informações* → *Executar assim mesmo* → abra um terminal **novo**.
- **macOS:** abra o `.pkg` (não assinado → **clique com o botão direito → Abrir**) → siga o assistente → abra um terminal **novo**.
- **Linux:** coloque o `install.sh` e o `.deb` na mesma pasta → `./install.sh`.

Depois, em qualquer projeto: **`mustard init`**.

> ⚠️ Os instaladores **não são assinados** — Windows (SmartScreen) e macOS (Gatekeeper) pedem uma confirmação na primeira execução; é esperado. O **passo a passo completo de cada sistema** está nos **Assets** abaixo (`TUTORIAL-WINDOWS.md`, `TUTORIAL-MACOS.md`, `TUTORIAL-LINUX.md`).

---
