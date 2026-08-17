# Notebook — fix/fix-linux-install-docs-make

> What surfaced during this work and does NOT belong to `fix-linux-install-docs-make`. What belongs
> to the spec amends the spec; this file is everything else, and once the pull
> request opens it is the next cycle's prompt.

- TUTORIAL-WINDOWS.md:82 e TUTORIAL-MACOS.md:92 carregam o MESMO defeito que esta unidade corrigiu no Linux: afirmam que apos 'mustard init' o trabalho esta feito e listam /mustard:* sem nunca mandar instalar o plugin. Ambos sao assets publicados e apontados pelo RELEASE-BODY novo.
- install.sh entrega um .deb a root com apenas 'nao-vazio' + dpkg-deb. O GitHub publica um digest sha256 por asset (a rota manual ja manda conferir), mas o caminho automatico nao o consome. Consumir exige a API, que a spec rejeitou por rate-limit — decisao a reabrir numa unidade propria.
- mustard.json bump 0.1.34->0.1.35 entrou dentro do commit da onda 1 (96f3ac1c) em vez de um commit de release, e o campo pertence ao auto-bump da main (86edfa33).
