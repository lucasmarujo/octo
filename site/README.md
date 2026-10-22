# orkai.marujo.dev

Landing page do orkai: HTML, CSS e JavaScript estáticos, sem build e sem nenhum recurso de terceiros (fontes,
vídeo e imagens ficam em `assets/`). A simulação da TUI em `assets/tui.js` reproduz a lógica de
`src/tui` (tempos, cores por projeto, glifos, título da janela e avisos), e o mascote é o sprite de
`src/tui/sprite.rs`.

## Testes

```sh
cd site
node --test
```

Os testes comparam a página com a fonte da verdade do repositório: o sprite e a paleta da TUI, a tabela de
agentes, os comandos de instalação e os binários do `README.md`. Ao mudar algum deles, atualize a página junto.
A página é em inglês, como o `README.md`, então a tabela de agentes é igual à do README. O `README.pt-BR.md` é a
tradução dele e os testes conferem que os dois têm o mesmo vídeo, a mesma estrutura e âncoras válidas.

## Deploy

Na VPS, o site roda em `/opt/orkai` como o container `orkai-site` (nginx na rede `proxy`), atrás do Caddy em
`/opt/proxy` com o bloco `orkai.marujo.dev`.

```sh
tar -C site --exclude=tests --exclude=README.md -czf - . | ssh lucas@76.13.167.76 'tar -xzf - -C /opt/orkai'
ssh lucas@76.13.167.76 'cd /opt/orkai && docker compose up -d'
```

O HTML, o CSS e o JS são revalidados a cada visita (`no-cache` com ETag); fontes, vídeo e imagens ficam 30 dias
em cache, então troque o nome do arquivo ao substituí-los.
