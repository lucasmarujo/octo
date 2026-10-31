# octo.marujo.dev

Landing page do Octo, o app desktop da OctoAI: HTML, CSS e JavaScript estáticos, sem build e sem nenhum recurso de
terceiros (fontes, vídeo e imagens ficam em `assets/`). O hero mostra a tela real do app (`assets/octo-board.webp` e
a versão 2x), e a seção de download aponta para os arquivos do Octo na release mais nova. O orkai CLI, agora legado,
tem a própria seção no fim da página: a simulação da TUI em `assets/tui.js` reproduz a lógica de `src/tui` (tempos,
cores por projeto, glifos, título da janela e avisos) e mostra o sprite de `src/tui/sprite.rs`, como o CLI. Os
mascotes do resto da página e o favicon são o Kai, o polvo do Octo, com as mesmas formas de `desktop/ui/js/kai.js`,
montado em camadas que só animam `transform` e `opacity` para rodar a 60 fps.

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
