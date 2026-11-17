// Graft's telemetry collector: a Cloudflare Worker over D1 (ADR 0035).
//
// POST /v1/events   batches from `graft __telemetry-send` (schema 1, ADR 0034)
// GET  /v1/summary  totals, for the owner only (Authorization: Bearer READ_TOKEN)
//
// Privacy: every event is checked against an allow-list of fields and
// values, then only added to daily counts. Raw events, IP addresses and
// headers are never stored or logged.

/** Largest body accepted. A full client queue (1,000 events) is ~220 KB. */
export const MAX_BODY = 512 * 1024;
/** Most events per batch (the client's queue cap). */
export const MAX_EVENTS = 1000;
/** Oldest day accepted: queued events may wait while an endpoint is down. */
export const MAX_AGE_DAYS = 31;

const DURATIONS = new Set(["<100ms", "<1s", "<10s", "<1m", ">=1m"]);
const OSES = new Set(["linux", "macos"]);
const ARCHES = new Set(["x86_64", "aarch64"]);
const VIAS = new Set(["cli", "mcp"]);
const FIELDS = ["arch", "code", "command", "day", "duration", "ok", "os", "v", "version", "via"];
const COMMAND = /^[a-z]{1,20}$/;
const CODE = /^[a-z_]{1,40}$/;
const VERSION = /^\d{1,4}\.\d{1,4}\.\d{1,4}(-[0-9A-Za-z.]{1,20})?$/;
const DAY = /^\d{4}-\d{2}-\d{2}$/;

const UPSERT = `INSERT INTO daily_counts
  (day, command, ok, code, duration, version, os, arch, via, count)
  VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
  ON CONFLICT (day, command, ok, code, duration, version, os, arch, via)
  DO UPDATE SET count = count + excluded.count`;

const json = (status, body) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });

/** Days between `day` (YYYY-MM-DD) and `now`, or NaN if not a real date. */
function ageInDays(day, now) {
  const t = Date.parse(`${day}T00:00:00Z`);
  if (Number.isNaN(t) || new Date(t).toISOString().slice(0, 10) !== day) return NaN;
  const today = Date.parse(now.toISOString().slice(0, 10) + "T00:00:00Z");
  return (today - t) / 86_400_000;
}

/**
 * The event as a row key, or null if it is not exactly a schema-1 event.
 * Anything unexpected (an extra field, an unknown value) drops the event,
 * so nothing outside the allow-list can be stored.
 */
export function validEvent(e, now = new Date()) {
  if (e === null || typeof e !== "object" || Array.isArray(e)) return null;
  const keys = Object.keys(e).sort();
  if (keys.length !== FIELDS.length || keys.some((k, i) => k !== FIELDS[i])) return null;
  if (e.v !== 1 || typeof e.ok !== "boolean") return null;
  if (typeof e.command !== "string" || !COMMAND.test(e.command)) return null;
  if (e.ok ? e.code !== null : typeof e.code !== "string" || !CODE.test(e.code)) return null;
  if (!DURATIONS.has(e.duration) || !OSES.has(e.os) || !ARCHES.has(e.arch) || !VIAS.has(e.via))
    return null;
  if (typeof e.version !== "string" || !VERSION.test(e.version)) return null;
  if (typeof e.day !== "string" || !DAY.test(e.day)) return null;
  const age = ageInDays(e.day, now);
  if (!(age >= -1 && age <= MAX_AGE_DAYS)) return null; // allow a day of clock skew
  return [e.day, e.command, e.ok ? 1 : 0, e.code ?? "", e.duration, e.version, e.os, e.arch, e.via];
}

/** Counts of identical valid events, and how many were dropped. */
export function aggregate(events, now = new Date()) {
  const counts = new Map();
  let dropped = 0;
  for (const e of events) {
    const key = validEvent(e, now);
    if (key === null) {
      dropped++;
      continue;
    }
    const k = JSON.stringify(key);
    const row = counts.get(k);
    if (row) row.count++;
    else counts.set(k, { key, count: 1 });
  }
  return { rows: [...counts.values()], dropped };
}

async function ingest(request, env) {
  if (!(request.headers.get("content-type") ?? "").startsWith("application/json"))
    return json(415, { error: "expected application/json" });
  const declared = Number(request.headers.get("content-length") ?? 0);
  if (declared > MAX_BODY) return json(413, { error: "batch too large" });
  const text = await request.text();
  if (text.length > MAX_BODY) return json(413, { error: "batch too large" });
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return json(400, { error: "invalid JSON" });
  }
  if (body?.schema !== 1 || !Array.isArray(body.events))
    return json(400, { error: "expected {schema: 1, events: [...]}" });
  if (body.events.length > MAX_EVENTS) return json(413, { error: "too many events" });
  const { rows, dropped } = aggregate(body.events);
  if (rows.length > 0) {
    await env.DB.batch(rows.map((r) => env.DB.prepare(UPSERT).bind(...r.key, r.count)));
  }
  // 2xx even with dropped events: retrying them would not make them valid.
  return json(202, { accepted: body.events.length - dropped, dropped });
}

/** Constant-time comparison, so the token cannot be guessed byte by byte. */
function sameToken(a, b) {
  const x = new TextEncoder().encode(a);
  const y = new TextEncoder().encode(b);
  let diff = x.length ^ y.length;
  for (let i = 0; i < Math.max(x.length, y.length); i++) diff |= (x[i] ?? 0) ^ (y[i] ?? 0);
  return diff === 0;
}

async function summary(request, env, url) {
  const token = env.READ_TOKEN ?? "";
  const given = (request.headers.get("authorization") ?? "").replace(/^Bearer /, "");
  if (token.length < 16 || !sameToken(given, token)) return json(401, { error: "unauthorized" });
  const days = Math.min(Math.max(Number(url.searchParams.get("days")) || 30, 1), 366);
  const since = new Date(Date.now() - days * 86_400_000).toISOString().slice(0, 10);
  const q = (sql) => env.DB.prepare(sql).bind(since).all().then((r) => r.results);
  const [byCommand, errors, platforms, versions] = await Promise.all([
    q(`SELECT command, via, SUM(count) AS runs,
              SUM(CASE WHEN ok = 0 THEN count ELSE 0 END) AS failed
       FROM daily_counts WHERE day >= ? GROUP BY command, via ORDER BY runs DESC`),
    q(`SELECT command, code, version, SUM(count) AS n
       FROM daily_counts WHERE day >= ? AND ok = 0
       GROUP BY command, code, version ORDER BY n DESC LIMIT 100`),
    q(`SELECT os, arch, SUM(count) AS runs
       FROM daily_counts WHERE day >= ? GROUP BY os, arch ORDER BY runs DESC`),
    q(`SELECT version, SUM(count) AS runs
       FROM daily_counts WHERE day >= ? GROUP BY version ORDER BY runs DESC`),
  ]);
  return json(200, { since, by_command: byCommand, errors, platforms, versions });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    try {
      if (url.pathname === "/v1/events" && request.method === "POST") return await ingest(request, env);
      if (url.pathname === "/v1/summary" && request.method === "GET")
        return await summary(request, env, url);
      return json(404, { error: "not found" });
    } catch {
      // No details: they could echo request contents into Cloudflare's logs.
      return json(500, { error: "internal error" });
    }
  },
};
