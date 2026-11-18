# Graft telemetry collector

A Cloudflare Worker that receives Graft's opt-in telemetry (ADR 0034) and
keeps **daily counts only** in a D1 (SQLite) database (ADR 0035). It runs
on Cloudflare's free plan.

- `POST /v1/events` takes the batches `graft` sends. Each event is checked
  against an allow-list of fields and values; anything else is dropped. A
  valid event only increments a count, such as "on 2026-11-17, `graft new`
  on macOS/aarch64 v0.1.1 failed with `not_found`: 3".
- `GET /v1/summary?days=30` returns totals per command, the most common
  error codes, platforms and versions. It needs `Authorization: Bearer
  <READ_TOKEN>`, so only you can read it.
- Nothing else is kept: no raw events, IP addresses, headers or request
  logs (`[observability] enabled = false`).

## Deploy on a free Cloudflare account

You need a Cloudflare account (free at <https://dash.cloudflare.com/sign-up>)
and `wrangler`, Cloudflare's CLI. Run it with `bunx wrangler` or
`npx wrangler`; nothing needs installing globally. All commands below run
in this directory.

1. **Log in.** This opens a browser to authorise wrangler:

   ```sh
   bunx wrangler login
   ```

2. **Create the database:**

   ```sh
   bunx wrangler d1 create graft-telemetry
   ```

   It prints a `database_id`. Put it in `wrangler.toml` in place of
   `REPLACE_WITH_YOUR_DATABASE_ID`. The ID is not a secret, but it is your
   account's, so keep the edit local if you'd rather not commit it.

3. **Create the table:**

   ```sh
   bunx wrangler d1 execute graft-telemetry --remote --file schema.sql
   ```

4. **Set the read token**, the password for `/v1/summary`. Generate one and
   save it in your password manager:

   ```sh
   openssl rand -hex 32            # copy the output
   bunx wrangler secret put READ_TOKEN   # paste it when asked
   ```

5. **Deploy:**

   ```sh
   bunx wrangler deploy
   ```

   It prints the Worker's URL, such as
   `https://graft-telemetry.<your-subdomain>.workers.dev`.

6. **Check it** with one test event:

   ```sh
   URL=https://graft-telemetry.<your-subdomain>.workers.dev
   curl -s -X POST "$URL/v1/events" -H 'content-type: application/json' \
     -d '{"schema":1,"events":[{"v":1,"day":"'"$(date -u +%F)"'","command":"doctor","ok":true,"code":null,"duration":"<1s","version":"0.1.1","os":"macos","arch":"aarch64","via":"cli"}]}'
   # {"accepted":1,"dropped":0}
   curl -s "$URL/v1/summary?days=7" -H "authorization: Bearer <your token>"
   ```

7. **Point Graft at it**, in `$GRAFT_HOME/config.toml`:

   ```toml
   [telemetry]
   endpoint = "https://graft-telemetry.<your-subdomain>.workers.dev/v1/events"
   ```

   Then `graft telemetry on`, run any command, and `graft telemetry send`
   (otherwise it sends by itself at most once a day).

## Reading the data

- **Summary as JSON:** `curl -s "$URL/v1/summary?days=30" -H "authorization:
  Bearer <token>"`. `days` goes up to 366.
- **Any SQL you like:**

  ```sh
  bunx wrangler d1 execute graft-telemetry --remote --command \
    "SELECT command, SUM(count) AS runs, SUM(CASE WHEN ok=0 THEN count END) AS failed
     FROM daily_counts GROUP BY command ORDER BY runs DESC"
  ```

- **In the browser:** Cloudflare dashboard, then Storage & Databases, D1,
  `graft-telemetry`, Console.

## Free-plan limits

At the time of writing, the free plan gives Workers about 100,000 requests
a day, and D1 5 GB of storage, 5 million rows read and 100,000 rows written
a day. Check <https://developers.cloudflare.com/workers/platform/pricing/>
and <https://developers.cloudflare.com/d1/platform/pricing/> for current
numbers. Each machine sends at most one batch a day, and a batch writes one
row per distinct combination, not per event, so a few thousand users stay
far inside these limits. Over a limit, requests fail and the clients keep
their events for the next day.

## Things to know

- **Anyone can post to the endpoint.** An open-source client cannot hold a
  secret. The allow-list limits what fake data can look like, but not how
  much of it there is. If that ever matters, add a rate-limiting rule (the
  free plan includes one) under Security, WAF, Rate limiting rules, on the
  `/v1/events` path.
- **Cloudflare still sees IP addresses** in transit, as any host does. This
  Worker never reads or stores them, and its request logs are off.
- **Graft has no default endpoint.** To ship this URL as the default in a
  release, write a new ADR (ADR 0034 requires one). Users still have to opt
  in either way.

## Tests

```sh
node --test    # or: bun test
```

The tests use a stand-in for D1, so they need no account or network.
