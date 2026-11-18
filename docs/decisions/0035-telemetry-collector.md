# 0035: Telemetry collector
Status: accepted

## Context

ADR 0034's client sends opted-in events to `[telemetry] endpoint`, but
there was no service to receive them. The owner asked for free hosting
only, and for no more data than necessary.

## Options considered

1. **A Vercel function with Postgres (Neon) or KV.** Free, but Vercel's
   free Hobby plan is for non-commercial use only.
2. **Supabase.** Free projects pause after about a week without activity,
   so telemetry would silently stop arriving.
3. **A Cloudflare Worker with D1 (SQLite).** The free plan allows
   commercial use and does not pause. Its limits (about 100k requests and
   100k rows written a day) are far above one batch per machine per day.
4. **A hosted product-analytics tool** (such as PostHog). Free tiers exist,
   but they are built around user identities and store raw events.

For storage: keep raw events, or keep only daily counts.

## Decision

Option 3, storing only daily counts. The code is in `telemetry/collector/`;
`README.md` there gives the deploy steps.

- `POST /v1/events`: the body must be JSON, at most 512 KiB and 1,000
  events. Each event must have exactly the ten schema-1 fields, and every
  value is checked against an allow-list or pattern: command `[a-z]{1,20}`,
  code `[a-z_]{1,40}` (null when ok), one of the five durations, `linux` or
  `macos`, `x86_64` or `aarch64`, `cli` or `mcp`, a semver version, and a
  real date no more than 31 days old. Invalid events are dropped and
  counted in the reply. Valid ones are grouped, and each group increments
  `daily_counts` through an upsert in one D1 batch. The reply is 202.
- `GET /v1/summary`: needs `Authorization: Bearer READ_TOKEN`, a Worker
  secret of at least 16 characters compared in constant time. It returns
  per-command runs and failures, the top error codes by version, platforms
  and versions.
- Request logs are off (`[observability] enabled = false`). The Worker never
  reads the client's IP address, and error responses never echo the request.
- Tests (`node --test`, with a D1 stand-in) run in a new CI job on the
  runner's preinstalled Node. No npm packages are installed in CI.

## Consequences

- Graft has a free, private destination for telemetry, and only daily
  totals are ever stored.
- Without identifiers, the data counts usage, not users.
- The endpoint takes posts from anyone. The allow-list limits the shape of
  fake data, not its volume; a free rate-limiting rule can cap volume if
  needed.
- Graft still ships with no default endpoint. Making this Worker's URL the
  default needs a new ADR, and users still have to opt in.
