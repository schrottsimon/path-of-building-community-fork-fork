# pob-proxy

Character import from a public profile (the Import/Export tab, upstream's
"account name" import) reads pathofexile.com's character pages. Browsers may
not call them from another site (no CORS headers), so the web app fetches
them through this proxy.

It forwards only what that import needs, only with GET:

| Path | Used for |
| --- | --- |
| `/character-window/get-characters` | the account's characters |
| `/account/view-profile/<account>` | the account name's exact spelling |
| `/character-window/get-passive-skills` | a character's tree and jewels |
| `/character-window/get-items` | a character's items and skills |

The caller's cookies and headers are never forwarded, so it cannot act as a
logged in user (`POESESSID` is never used). GGG's status codes and rate
limit headers are passed back; successful responses are cached briefly.
Profiles and the characters tab must be public on pathofexile.com.

## Running it

```sh
cargo build --release -p proxy
POB_PROXY_ALLOW_ORIGIN=https://pob.example.com target/release/pob-proxy
```

| Variable | Default | |
| --- | --- | --- |
| `POB_PROXY_ALLOW_ORIGIN` | (required) | origin of the web app, or `*` |
| `POB_PROXY_BIND` | `127.0.0.1:8787` | address to listen on |
| `POB_PROXY_USER_AGENT` | `pob-web-proxy/<version>` | sent to GGG; name your instance and a contact |
| `POB_PROXY_CACHE_SECS` | `60` | how long successful responses are reused |
| `POB_PROXY_UPSTREAM` | `https://www.pathofexile.com` | for tests |

It speaks plain HTTP: put it behind the reverse proxy that serves the site
(TLS), for example with nginx:

```nginx
location /poe/ {
    proxy_pass http://127.0.0.1:8787/;
}
```

Then point the web app at it with `config.json` next to `index.html` (not
committed, see `web/public/config.json`):

```json
{ "poeProxy": "https://pob.example.com/poe" }
```

Served from the same origin as the site, CORS does not even come into play;
`POB_PROXY_ALLOW_ORIGIN` still has to be set.
