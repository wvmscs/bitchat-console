<!--
SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Behind a TLS proxy

The page shows private messages: off the loopback it travels in HTTPS only. Instead of a
certificate given to the console (`--tls-cert`, `--tls-key`), a proxy in front can do the TLS
and send the requests in clear HTTP to the console's port (decision D34):

```sh
bitchat-console u09tv --behind-proxy https://bitchat.example.org        # proxy on this machine
bitchat-console u09tv --listen 0.0.0.0:8090 --allow-remote \
    --behind-proxy https://bitchat.example.org --proxy-from 172.17.0.0/16  # proxy elsewhere
```

With `bitchat-launcher`, the console behind the proxy and the map behind the console:

```sh
bitchat-launcher u09tv --behind-proxy https://bitchat.example.org --port 8080
```

## What is checked

Every request must:

- come from the proxy's address: `--proxy-from` (addresses or networks, comma separated,
  repeatable; default `127.0.0.1/8,::1`, a proxy on the same machine), else `403
  not_from_proxy`;
- say that it reached the proxy in HTTPS: `X-Forwarded-Proto: https`, `Forwarded: proto=https`
  or Cloudflare's `CF-Visitor: {"scheme":"https"}`; none of them saying anything else, else
  `403 not_https_upstream`;
- carry the public name in `Host` (and in `X-Forwarded-Host` when the proxy sets it), else
  `421 wrong_host`; commands (`POST`) the public origin in `Origin`, else `403 wrong_origin`.

At start, the console asks `https://<name>/.well-known/bitchat-proxy-check` and expects a token
drawn at this start: the proxy exists, does the TLS and leads to this console. Otherwise it
does not start. The probe connects directly (not through Tor: it goes to the console's own
name, never to a relay), checking the certificate against the web roots, plus those of
`SSL_CERT_FILE` when it is set (a local authority, such as mkcert's). `--no-proxy-check` skips
it, when the machine cannot reach its own public name (a router without NAT loopback) or when
the proxy asks for a login there.

The console has no login of its own: anyone who reaches the public name reads and writes as
you. Put an authentication in the proxy (Cloudflare Access, `auth_basic`, `basic_auth`), and
leave `/.well-known/bitchat-proxy-check` open, or use `--no-proxy-check`. Any program on the
proxy's machine can also reach the port as the proxy would.

The live updates are a stream (SSE): the proxy must not buffer them, and should keep idle
connections at least a minute (the console sends a comment every 15 s). In front of the
browser it speaks HTTP/2, so every open tab shares one connection.

## Cloudflare Tunnel

`cloudflared` connects out to Cloudflare, which serves the name in HTTPS: no open port, no
certificate to renew. It reaches the console from this machine (the default `--proxy-from`),
and sets `X-Forwarded-Proto`, `CF-Visitor` and the public `Host`.

```sh
brew install cloudflared            # or the package of your system
cloudflared tunnel login
cloudflared tunnel create bitchat
cloudflared tunnel route dns bitchat bitchat.example.org
```

`~/.cloudflared/config.yml`:

```yaml
tunnel: bitchat
credentials-file: /Users/you/.cloudflared/<tunnel-id>.json
ingress:
  - hostname: bitchat.example.org
    service: http://127.0.0.1:8080
  - service: http_status:404
```

```sh
cloudflared tunnel ingress validate
cloudflared tunnel run bitchat
bitchat-launcher u09tv --behind-proxy https://bitchat.example.org --port 8080
```

Protect the name with Cloudflare Access (Zero Trust, *Access → Applications*, a self-hosted
application for `bitchat.example.org`), and add a *Bypass* policy for the path
`/.well-known/bitchat-proxy-check`, or start with `--no-proxy-check`.

Without a tunnel (a DNS record proxied by Cloudflare to a public address of yours), use
*Full (strict)* TLS mode with a certificate on an NGINX or Caddy in front, as below: Cloudflare
would otherwise reach your machine in clear HTTP across the Internet.

## NGINX

```nginx
server {
    listen 443 ssl;
    http2 on;
    server_name bitchat.example.org;
    ssl_certificate     /etc/letsencrypt/live/bitchat.example.org/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/bitchat.example.org/privkey.pem;

    # auth_basic "bitchat"; auth_basic_user_file /etc/nginx/bitchat.htpasswd;
    # location = /.well-known/bitchat-proxy-check { auth_basic off; proxy_pass http://127.0.0.1:8080; }

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $http_host;             # the public name, with its port if any
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_buffering off;                          # live updates (SSE)
        proxy_read_timeout 1h;
    }
}
server {                                              # HTTP to HTTPS
    listen 80;
    server_name bitchat.example.org;
    return 301 https://$host$request_uri;
}
```

`nginx -t`, then `nginx -s reload`; certificate with `certbot --nginx -d bitchat.example.org`.

## Caddy

Caddy gets and renews the certificate itself, speaks HTTP/2, and sets `X-Forwarded-Proto` and
the public `Host`. `Caddyfile`:

```caddyfile
bitchat.example.org {
	# basic_auth { you <hash from `caddy hash-password`> }
	reverse_proxy 127.0.0.1:8080 {
		flush_interval -1   # live updates (SSE)
	}
}
```

`caddy validate --config Caddyfile`, then `caddy run --config Caddyfile` (or
`caddy reload`). With `basic_auth`, put it in a `route` that leaves
`/.well-known/bitchat-proxy-check` out, or start with `--no-proxy-check`.

## Proxy on another machine or in a container

The console then listens beyond the loopback, and accepts only the proxy's addresses:

```sh
bitchat-console u09tv --listen 0.0.0.0:8090 --allow-remote \
    --behind-proxy https://bitchat.example.org --proxy-from 10.0.0.2
```

Keep that port closed to everything else (firewall): the requests between the proxy and the
console travel in clear.

## Tested

On 29/10/2026, on macOS with a mkcert certificate: Caddy 2.11.7 and NGINX 1.31.8 with the files
above (HTTP/2 to the browser), the launcher behind Caddy: start probe passed, page, map and
commands from Chrome, live streams, direct access to the port refused. The `cloudflared`
2026.10.5 file above passes `ingress validate`.
