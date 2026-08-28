Mustard — instalador
====================

UM instalador por sistema, completo: traz o CLI E o Mustard Dashboard juntos.
O Dashboard é um SERVIDOR: ele abre uma porta na máquina e o painel aparece no
navegador. Você NÃO precisa instalar Rust nem compilar nada — já vem pronto.

  LINUX (Ubuntu): instale numa linha com o install.sh (ele mesmo baixa o
                  mustard_<versao>_amd64.deb do Release e chama o apt)
  WINDOWS:        Mustard_<versao>_x64-setup.exe
  macOS:          Mustard-<versao>-universal.pkg  (Intel + Apple Silicon)


Requisitos
----------
- Ubuntu 22.04+ (glibc 2.35+) — é a glibc contra a qual os binários são
  compilados. Não há mais dependência de biblioteca gráfica alguma.
- Windows 10/11.
- macOS 11+ (Big Sur ou mais novo).
- Em todos: Claude Code instalado e logado; nenhum toolchain de dev é necessário.


Como instalar
-------------

LINUX (Ubuntu) — rota recomendada: uma linha, sem baixar nada à mão
  Instalar tudo:
    curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh | sh
  Instalar E já preparar um projeto (o caminho vai depois do  -s -- ):
    curl -fsSL https://github.com/rubensrpj/mustard/releases/latest/download/install.sh | sh -s -- /caminho/do/projeto

LINUX (Ubuntu) — alternativa manual (permite conferir o sha256 antes)
  1. Baixe dos Assets do Release o install.sh e o mustard_<versao>_amd64.deb
     para a mesma pasta e entre nela.
  2. Confira o pacote:  sha256sum mustard_<versao>_amd64.deb
     (compare com o "digest" que a página do Release mostra para esse asset)
  3. Dê permissão de execução ao script — os assets do Release chegam SEM ela,
     e sem esse passo o shell responde "Permission denied":
       chmod +x install.sh
  4a. Instalar tudo:                        ./install.sh
  4b. Instalar E já preparar um projeto:    ./install.sh /caminho/do/projeto
  Com um .deb ao lado, o install.sh usa esse arquivo e não baixa nada.
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
- LINUX:   o apt instala o CLI e o mustard-dashboard em /usr/lib/mustard/bin
           (atalhos em /usr/bin), com os arquivos da tela ao lado do servidor;
           adiciona o atalho "Mustard Dashboard" ao menu de aplicativos, e esse
           atalho INICIA O SERVIDOR num terminal.
- WINDOWS: instala os binários (com os templates e os arquivos da tela) na pasta
           do programa, adiciona o CLI ao PATH e cria o atalho no Menu Iniciar,
           que INICIA O SERVIDOR numa janela de console.
- macOS:   instala tudo em /usr/local/mustard e cria os atalhos no PATH
           (/usr/local/bin). Não há .app: o painel se abre no navegador.
- Em todos: depois é só rodar `mustard init` num projeto para criar o .claude/ e
  instalar o plugin dentro do Claude Code (veja "Como usar depois").


Como usar depois
----------------
- Prepare um projeto:  cd <projeto> && mustard init
- Instale o plugin DENTRO do Claude Code (o instalador do sistema traz só os
  binários e os templates; os comandos /mustard:*, os hooks e o MCP de memória
  vêm do plugin). Abra o Claude Code no projeto e digite:
      /plugin marketplace add rubensrpj/mustard
      /plugin install mustard@mustard-local
  O "@mustard-local" é o NOME do marketplace, não um caminho. Recarregue o
  Claude Code (feche e abra) para os hooks entrarem.
  Se aparecer  Plugin "mustard" not found in any marketplace , faltou o primeiro
  comando; se o add falhar ao clonar, use a URL completa do repositório:
      /plugin marketplace add https://github.com/rubensrpj/mustard.git
- Rode o Claude Code normalmente. Os hooks do Mustard vêm do plugin instalado no
  passo acima, não do `mustard init`: o init só escreve o .claude/ e o
  mustard.json, e o .claude/settings.json que ele grava não traz hook nenhum.
- Versão instalada:  mustard --version   /   mustard-rt --version
- Dashboard: rode  mustard-dashboard  na pasta onde ficam seus projetos (a
  varredura começa no diretório de onde o servidor foi iniciado). Ele imprime
  http://127.0.0.1:7777/ e abre o navegador quando há tela. No Linux e no
  Windows o atalho "Mustard Dashboard" do menu faz o mesmo.
  Sem --host o painel só responde na própria máquina, de propósito (ele lê o
  .claude/ de TODOS os seus projetos). Para alcançar de outro computador:
      mustard-dashboard --host 0.0.0.0
  e acesse http://<ip-da-maquina>:7777/ . Outra porta: --port ou a variável
  MUSTARD_DASHBOARD_PORT; porta ocupada não é erro, ele usa a próxima livre.


Como remover
------------
- LINUX:   sudo apt remove mustard
- WINDOWS: desinstale "Mustard" em Aplicativos (Painel de Controle).
- macOS:   apague /usr/local/mustard e rode:  sudo rm /usr/local/bin/mustard*
                                                     /usr/local/bin/scan /usr/local/bin/rtk
- Em um projeto testado, a pasta .claude/ pode ser apagada à vontade.
