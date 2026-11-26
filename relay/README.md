# octo-relay

O endereço fixo de cada computador com o Octo. O túnel grátis da Cloudflare (`*.trycloudflare.com`) ganha um
endereço novo sempre que o computador fica desligado por alguns minutos; este serviço dá a cada computador um
endereço que nunca muda, `https://<id>.octo.marujo.dev`, e só responde nele com um redirecionamento `307` para o
túnel do momento. Nenhum tráfego do celular passa por aqui: depois do redirecionamento a conexão segue direto pela
Cloudflare, cifrada de ponta a ponta com a chave do QR code, que nunca chega a este serviço.

Node puro, sem dependências: `relay.mjs` tem as regras e `server.mjs` a rede e o arquivo de dados.

## Como funciona

- `PUT https://octo-relay.marujo.dev/v1/computers/<id>` com `Authorization: Bearer <token>` e o corpo
  `{"url":"https://<nome>.trycloudflare.com"}`. O `id` são 16 bytes aleatórios em hexadecimal e o `token` 32 bytes,
  os dois criados no computador (`src/pwa/relay.rs`). O primeiro pedido prende o `id` ao SHA-256 do `token`; depois
  disso só quem tem o `token` muda o destino, então ninguém toma o endereço de outro computador.
- Só um endereço `https://<nome>.trycloudflare.com` é aceito como destino, então o serviço não redireciona para
  nenhum outro site.
- `https://<id>.octo.marujo.dev/<caminho>` responde `307` para `<túnel>/<caminho>`, também para o pedido de
  WebSocket do celular. Um `id` desconhecido responde `404`.
- `GET /ask?domain=<host>`, só na rede interna, é a pergunta do Caddy antes de emitir o certificado de um endereço:
  a resposta é sim apenas para um `id` registrado.
- Limites: 20 computadores novos por rede a cada 24 horas, 200 mil computadores no total, uma mudança de destino
  por segundo por computador e corpo de até 1 KB.
- O serviço não escreve IP em lugar nenhum. O arquivo de dados guarda, por computador, o `id`, o SHA-256 do `token`,
  o endereço do túnel e a hora da última mudança.

## Testes

```sh
node --test relay/tests/relay.test.mjs
```

## Deploy

Na VPS o serviço roda em `/opt/octo-relay` como o container `octo-relay` (`node:22-alpine` na rede `proxy`), atrás do
Caddy em `/opt/proxy`. Os dados ficam em `/opt/octo-relay/data/computers.jsonl`: apagar esse arquivo desfaz o
endereço de todos os computadores, e cada celular teria de ser pareado de novo.

```sh
tar -C relay --exclude=tests --exclude=README.md -czf - . | ssh lucas@76.13.167.76 'tar -xzf - -C /opt/octo-relay'
ssh lucas@76.13.167.76 'cd /opt/octo-relay && docker compose up -d --force-recreate'
```

O `.env` em `/opt/octo-relay` define `RELAY_UID` e `RELAY_GID` com o usuário dono da pasta `data`.

No Caddyfile, a opção global diz a quem perguntar antes de emitir um certificado sob demanda, e dois blocos
servem o registro e os endereços:

```
{
	on_demand_tls {
		ask http://octo-relay:8080/ask
	}
}

octo-relay.marujo.dev {
	header Strict-Transport-Security "max-age=31536000"
	reverse_proxy octo-relay:8080
}

*.octo.marujo.dev {
	tls {
		on_demand
	}
	reverse_proxy octo-relay:8080
}
```

O bloco é `*.octo.marujo.dev`, e não `*.marujo.dev`, de propósito: a política sob demanda vale para todo nome que
o curinga cobre, e os outros sites de `marujo.dev` passariam a depender da resposta deste serviço para renovar o
certificado. Cada computador recebe um certificado próprio na primeira vez que o endereço é pedido; a Let's Encrypt
emite 50 por semana para `marujo.dev` inteiro e o Caddy usa a ZeroSSL quando esse limite acaba.
