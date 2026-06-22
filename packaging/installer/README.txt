Mustard — instalador
====================

UM instalador por sistema, completo: traz o CLI E o Mustard Dashboard juntos.
Você NÃO precisa instalar Rust nem compilar nada — já vem pronto.

  LINUX (Ubuntu): mustard_<versao>_amd64.deb  (via apt, com install.sh)
  WINDOWS:        Mustard Dashboard_<versao>_x64-setup.exe
  macOS:          Mustard-<versao>-universal.pkg  (Intel + Apple Silicon)


Requisitos
----------
- Ubuntu 22.04+ (glibc 2.35+). O Dashboard depende do webkit2gtk-4.1, que o apt
  instala sozinho; ele não existe no Ubuntu 20.04.
- Windows 10/11.
- macOS 11+ (Big Sur ou mais novo).
- Em todos: Claude Code instalado e logado; nenhum toolchain de dev é necessário.


Como instalar
-------------

LINUX (Ubuntu)
  1. Coloque o install.sh e o mustard_*.deb na mesma pasta e entre nela.
  2a. Instalar tudo:                        ./install.sh
  2b. Instalar E já preparar um projeto:    ./install.sh /caminho/do/projeto
  (Equivale a:  sudo apt install ./mustard_<versao>_amd64.deb)

WINDOWS
  1. Dê duplo-clique no ...-setup.exe e siga o assistente.
  2. Abra um NOVO terminal (o CLI entra no PATH na instalação).
  Obs.: como o instalador não é assinado, o SmartScreen pode avisar — clique em
  "Mais informações" > "Executar assim mesmo".

macOS
  1. Dê duplo-clique no Mustard-<versao>-universal.pkg e siga o assistente.
  2. Abra um NOVO terminal (o CLI entra no PATH na instalação).
  Obs.: como o pacote não é assinado/notarizado, o macOS pode recusar na 1ª vez —
  clique-direito no .pkg > Abrir, ou autorize em Ajustes > Privacidade e Segurança.


O que cada instalador faz
-------------------------
- LINUX:   o apt instala o CLI em /usr/lib/mustard/bin (atalhos em /usr/bin) e o
           Dashboard, resolvendo as dependências de sistema; adiciona o atalho
           "Mustard Dashboard" ao menu de aplicativos.
- WINDOWS: instala o Dashboard + os binários do CLI (com os templates) na pasta
           do programa, adiciona o CLI ao PATH e cria o atalho no Menu Iniciar.
- macOS:   instala o "Mustard Dashboard.app" em /Applications (com o CLI e os
           templates embutidos) e cria os atalhos do CLI no PATH (/usr/local/bin).
- Em todos: depois é só rodar `mustard init` num projeto para criar o .claude/.


Como usar depois
----------------
- Prepare um projeto:  cd <projeto> && mustard init
- Rode o Claude Code normalmente — os hooks do Mustard já ficam ligados via
  .claude/settings.json.
- Versão instalada:  mustard --version   /   mustard-rt --version
- Dashboard: "Mustard Dashboard" no menu (Linux) / Launchpad (macOS) / Menu
  Iniciar (Windows).


Como remover
------------
- LINUX:   sudo apt remove mustard
- WINDOWS: desinstale "Mustard Dashboard" em Aplicativos (Painel de Controle).
- macOS:   arraste o app para o Lixo e rode:  sudo rm /usr/local/bin/mustard*
                                                     /usr/local/bin/scan /usr/local/bin/rtk
- Em um projeto testado, a pasta .claude/ pode ser apagada à vontade.
