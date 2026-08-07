---
name: planpool
description: Share an HTML plan, report, or any rendered document as an expiring shareable URL using the pp CLI. Use when the user asks to share or publish a plan, wants a link to a rendered HTML document, or mentions planpool or pp.
---

# Sharing plans with planpool (`pp`)

planpool hosts single-file HTML documents at unguessable URLs that expire
after a TTL. The `pp` CLI uploads a file (or stdin) and prints the shareable
URL on stdout.

## Prerequisites

`pp` must be on PATH and configured through the environment:

- `PLANPOOL_URL` — server base URL (or pass `--url <url>` to any command)
- `PLANPOOL_TOKEN` — bearer token; required for push/delete, while viewing a
  plan needs only its URL

Verify once before first use:

```sh
pp health
```

Success prints `server ok` and `token ok` to stderr and exits 0. If it fails,
report the problem to the user instead of retrying — the server is down, the
URL is wrong, or the token is missing/invalid.

## Commands

```sh
pp push plan.html                 # upload, print plan URL on stdout
pp push plan.html --ttl 2h        # custom lifetime
generate-plan | pp push           # reads stdin when FILE is omitted or "-"
pp push plan.html --json          # print full JSON {id, url, created_at, expires_at}
pp delete <id-or-url>             # retract a plan early
pp open <id-or-url>               # open a plan in the user's browser
```

`--ttl` accepts plain seconds (`3600`) or humantime (`30m`, `1h`, `7days`).
`<id-or-url>` is either the 32-char hex ID or the full plan URL.

Only the result (URL or JSON) goes to stdout; all status notes and errors go
to stderr. Exit code is 0 on success, 1 on any failure — so
`URL=$(pp push plan.html)` and pipes are safe.

## Authoring plans

- Produce one self-contained HTML file: inline all CSS/JS, no references to
  local files. The server stores exactly one file per plan.
- Anyone holding the URL can view the plan. Never include secrets, tokens, or
  private data beyond what the user intends to share.
- Uploads are size-limited (5 MB by default).

## TTL behavior

- Omitting `--ttl` uses the server default (typically 7 days); requested TTLs
  are clamped to a server-side maximum (typically 30 days).
- The actual lifetime is echoed on stderr (`expires in …`) and returned in
  `--json` output (`expires_at`, unix seconds) — trust that over the value
  you requested.

## Typical workflow

1. Write the plan as a self-contained HTML file.
2. Run `pp push <file> --ttl <ttl>` and capture stdout.
3. Give the user the URL together with when it expires.
