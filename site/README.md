# octo.marujo.dev

Landing page do Octo, o app desktop da OctoAI: HTML, CSS e JavaScript estáticos, sem build e sem nenhum recurso de
terceiros (fontes, vídeo e imagens ficam em `assets/`). O hero mostra a tela real do app (`assets/octo-board.webp` e
a versão 2x); o problema e o tour de recursos mostram telas do Octo exportadas do Paper (`assets/octo-*.webp`, 1x e 2x), e a seção de download aponta para os arquivos do Octo na release mais nova. O orkai CLI, agora legado,
tem a própria seção no fim da página: a simulação da TUI em `assets/tui.js` reproduz a lógica de `src/tui` (tempos,
cores por projeto, glifos, título da janela e avisos) e mostra o sprite de `src/tui/sprite.rs`, como o CLI. Os
mascotes do resto da página e a imagem de compartilhamento são o Kai pixel do Octo: as tiras em
`assets/kai/` são cópias exatas das de `desktop/ui/kai/`, uma pose por quadro, que só animam `transform` para rodar a 60 fps.
A cena do footer usa `assets/kai/ink.png`, uma tira de 14 quadros de 128 × 96: os quatro primeiros são o idle do app, pixel a pixel, e os seguintes mantêm a cabeça e o boné dele e só mudam a expressão (susto, olhar firme, olhos espremidos), com os oito tentáculos agarrando as bordas e a tinta desenhados para a cena. A tira sempre cobre a tela, então nenhum tentáculo aparece cortado.
O favicon e os ícones são o Kai espiando pelo canto da janela do Octo (a proposta B), em pixel art, o mesmo ícone do app. `favicon.svg` é a grade de 48 pixels, montada com o sprite idle do Kai em escala 1:1, e todo tamanho sai dela por vizinho mais próximo (o centro de cada pixel), sem suavização e só com as cores da grade.

## Testes

```sh
cd site
node --test
```

Os testes comparam a página com a fonte da verdade do repositório: o sprite e a paleta da TUI, o Kai do app, a tabela de
agentes, os comandos de instalação do CLI e as tabelas de downloads do Octo e do CLI no `README.md`. Ao mudar algum deles, atualize a página junto.
A página é em inglês, como o `README.md`, então a tabela de agentes é igual à do README. O `README.pt-BR.md` é a
tradução dele e os testes conferem que os dois têm o mesmo vídeo, a mesma estrutura e âncoras válidas.

## Deploy

Na VPS, o site roda em `/opt/octo` como o container `octo-site` (nginx na rede `proxy`), atrás do Caddy em
`/opt/proxy` com o bloco `octo.marujo.dev`.

```sh
tar -C site --exclude=tests --exclude=README.md -czf - . | ssh lucas@76.13.167.76 'tar -xzf - -C /opt/octo'
ssh lucas@76.13.167.76 'cd /opt/octo && docker compose up -d'
```

O HTML, o CSS e o JS são revalidados a cada visita (`no-cache` com ETag); fontes, vídeo e imagens ficam 30 dias
em cache, então troque o nome do arquivo ao substituí-los.
