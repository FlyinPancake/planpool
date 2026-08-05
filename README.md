# planpool

A small pool of expiring static HTML plans. AI agents `POST` a plan and get back a
shareable URL; the plan is served until its TTL runs out, then disappears.

No database, no UI — plans live as flat files in a directory, and each plan's
128-bit random ID doubles as its view capability (like a secret gist URL).

Interactive API docs (Scalar) are served at `/docs`, with the OpenAPI 3.1 spec
at `/api-docs/openapi.json`.

## Workspace layout

- `crates/planpool-server` — the HTTP server (binary name: `planpool`)
- `crates/planpool-cli` — the client CLI (binary name: `pp`)
- `crates/planpool-types` — request/response types shared by both

## Running

```sh
PLANPOOL_TOKEN=$(openssl rand -hex 32) cargo run --release
```

For development, [mise](https://mise.jdx.dev) tasks are set up with safe local
defaults (dev token, `127.0.0.1:8642`, `.dev-plans/` storage — override via
`.env`): `mise run dev` runs the server and restarts it on source changes
(`run-server`, `test`, `lint`, and `env-example` are also available).

The token is required and must be at least 16 characters. It protects uploads and
deletes; viewing needs only the (unguessable) plan URL.

### Configuration

| Env var                   | Default          | Meaning                                                                                                    |
| ------------------------- | ---------------- | ---------------------------------------------------------------------------------------------------------- |
| `PLANPOOL_TOKEN`          | _(required)_     | Bearer token for `POST` / `DELETE`                                                                         |
| `PLANPOOL_ADDR`           | `0.0.0.0:8080`   | Listen address                                                                                             |
| `PLANPOOL_DATA_DIR`       | `./plans`        | Where plan files are stored                                                                                |
| `PLANPOOL_DEFAULT_TTL`    | `7days`          | TTL when the upload doesn't specify one ([humantime](https://docs.rs/humantime) format, e.g. `12h`, `30m`) |
| `PLANPOOL_MAX_TTL`        | `30days`         | Requested TTLs are clamped to this (humantime format)                                                      |
| `PLANPOOL_MAX_BODY_BYTES` | `5242880` (5 MB) | Upload size limit                                                                                          |
| `PLANPOOL_PUBLIC_URL`     | _(Host header)_  | Base URL used in returned links, e.g. `https://plans.example.com`                                          |

A ready-to-fill [`.env.example`](.env.example) is checked in; regenerate it any
time with `planpool --env-example` (it's derived from the config struct itself,
so it can't drift). Note the server doesn't load `.env` files on its own — use
your process manager (docker compose and systemd `EnvironmentFile=` both read
them natively).

## The `pp` CLI

The client agents actually use. Configure `PLANPOOL_URL` and `PLANPOOL_TOKEN`
(the mise dev env sets both for the local dev server), then:

```sh
pp push plan.html --ttl 2h        # prints the plan URL on stdout
generate-plan | pp push           # reads stdin
pp push plan.html --json          # full response (id, url, timestamps)
pp delete <id-or-url>             # retract a plan early
pp open <id-or-url>               # open in browser
pp health                         # check server reachability + token validity
pp completions <shell>            # shell completions
```

`--ttl` accepts plain seconds or humantime (`1h`, `7days`); the CLI converts to
canonical seconds before sending. Only the result goes to stdout — everything
else (status notes, errors) goes to stderr — so `$(pp push …)` and pipes are
safe. Exit code is 0 on success, 1 on any failure.

## API

### Upload a plan

```sh
curl -X POST 'https://plans.example.com/plans?ttl=86400' \
  -H "Authorization: Bearer $PLANPOOL_TOKEN" \
  -H 'Content-Type: text/html' \
  --data-binary @plan.html
```

```json
{
  "id": "879255f0c80239b707ef77159a2d7980",
  "url": "https://plans.example.com/plans/879255f0c80239b707ef77159a2d7980",
  "created_at": 1785969984,
  "expires_at": 1785970104
}
```

`ttl` is in seconds and optional. Timestamps are unix seconds.

### View a plan

`GET /plans/{id}` — serves the HTML. Returns 404 once expired or deleted.

### Delete a plan early

```sh
curl -X DELETE "https://plans.example.com/plans/$ID" \
  -H "Authorization: Bearer $PLANPOOL_TOKEN"
```

Returns 204, or 404 if it's already gone.

### Health check

`GET /healthz` → `200 ok`.

### API docs

`GET /docs` — interactive Scalar UI. `GET /api-docs/openapi.json` — the raw
OpenAPI 3.1 spec, handy for generating clients.

## Retention

Expired plans 404 immediately, and a background task sweeps them off disk every
minute. Storage is one `{id}.html` + `{id}.json` sidecar pair per plan in
`PLANPOOL_DATA_DIR`, so the pool survives restarts and can be inspected with `ls`.

## Deployment notes

- Terminate TLS in a reverse proxy (Caddy, nginx, Traefik) and set
  `PLANPOOL_PUBLIC_URL` so returned links use the public origin.
- Plans are arbitrary agent-generated HTML served from this origin, so a
  malicious plan can run JavaScript there. Host planpool on its own
  (sub)domain with nothing else on it — no cookies, no admin UI.
- Logs go to stdout; control verbosity with `RUST_LOG` (e.g. `RUST_LOG=planpool=debug`).
