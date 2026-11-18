import { test } from "node:test";
import assert from "node:assert/strict";

import worker, { aggregate, validEvent, MAX_EVENTS } from "../src/index.js";

const today = new Date().toISOString().slice(0, 10);

/** A schema-1 event as `graft` writes it. */
const event = (over = {}) => ({
  v: 1,
  day: today,
  command: "new",
  ok: true,
  code: null,
  duration: "<1s",
  version: "0.1.1",
  os: "macos",
  arch: "aarch64",
  via: "cli",
  ...over,
});

/** A D1 stand-in: applies the upsert to a Map; answers summaries empty. */
function fakeDb() {
  const rows = new Map();
  return {
    rows,
    prepare(sql) {
      return {
        bind: (...args) => ({
          sql,
          args,
          all: async () => ({ results: [] }),
        }),
      };
    },
    async batch(stmts) {
      for (const { sql, args } of stmts) {
        assert.match(sql, /ON CONFLICT/);
        const key = JSON.stringify(args.slice(0, 9));
        rows.set(key, (rows.get(key) ?? 0) + args[9]);
      }
    },
  };
}

const post = (body, headers = {}) =>
  new Request("https://t.example/v1/events", {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: typeof body === "string" ? body : JSON.stringify(body),
  });

test("accepts exactly the schema-1 event", () => {
  assert.ok(validEvent(event()));
  assert.ok(validEvent(event({ ok: false, code: "not_found", via: "mcp" })));
});

test("drops anything outside the allow-list", () => {
  const bad = [
    event({ path: "/Users/someone/repo" }), // extra field
    (({ via, ...rest }) => rest)(event()), // missing field
    event({ command: "new --from secret" }),
    event({ command: "x".repeat(21) }),
    event({ ok: false, code: null }),
    event({ ok: true, code: "failed" }),
    event({ code: "Has Spaces", ok: false }),
    event({ duration: "3.2s" }),
    event({ os: "windows" }),
    event({ arch: "riscv64" }),
    event({ via: "web" }),
    event({ version: "1.0" }),
    event({ version: "1.0.0-" + "x".repeat(30) }),
    event({ day: "2026-02-30" }),
    event({ day: "1999-01-01" }), // too old
    event({ v: 2 }),
    null,
    [],
    "new",
  ];
  for (const e of bad) assert.equal(validEvent(e), null, JSON.stringify(e));
});

test("aggregates identical events into one count", () => {
  const { rows, dropped } = aggregate([
    event(),
    event(),
    event({ ok: false, code: "not_found" }),
    event({ os: "plan9" }),
  ]);
  assert.equal(dropped, 1);
  assert.deepEqual(
    rows.map((r) => r.count).sort(),
    [1, 2],
  );
});

test("stores daily counts and adds to them", async () => {
  const env = { DB: fakeDb() };
  for (let i = 0; i < 2; i++) {
    const res = await worker.fetch(post({ schema: 1, events: [event(), event({ via: "mcp" })] }), env);
    assert.equal(res.status, 202);
    assert.deepEqual(await res.json(), { accepted: 2, dropped: 0 });
  }
  assert.deepEqual([...env.DB.rows.values()].sort(), [2, 2]);
  // Nothing but the allow-listed fields reaches storage.
  for (const key of env.DB.rows.keys()) assert.equal(JSON.parse(key).length, 9);
});

test("rejects malformed batches", async () => {
  const env = { DB: fakeDb() };
  const status = async (req) => (await worker.fetch(req, env)).status;
  assert.equal(await status(post("not json")), 400);
  assert.equal(await status(post({ events: [] })), 400);
  assert.equal(await status(post({ schema: 1, events: Array(MAX_EVENTS + 1).fill(event()) })), 413);
  assert.equal(await status(post({ schema: 1, events: [] }, { "content-type": "text/plain" })), 415);
  assert.equal(await status(post("x".repeat(600 * 1024))), 413);
  assert.equal(env.DB.rows.size, 0);
});

test("the summary needs the read token", async () => {
  const env = { DB: fakeDb(), READ_TOKEN: "a-long-enough-secret-token" };
  const get = (auth) =>
    worker.fetch(
      new Request("https://t.example/v1/summary?days=7", {
        headers: auth ? { authorization: `Bearer ${auth}` } : {},
      }),
      env,
    );
  assert.equal((await get()).status, 401);
  assert.equal((await get("wrong-token-of-some-length")).status, 401);
  const ok = await get(env.READ_TOKEN);
  assert.equal(ok.status, 200);
  assert.ok("by_command" in (await ok.json()));
  // An unset or short token never opens it.
  const open = { DB: fakeDb(), READ_TOKEN: "" };
  const res = await worker.fetch(
    new Request("https://t.example/v1/summary", { headers: { authorization: "Bearer " } }),
    open,
  );
  assert.equal(res.status, 401);
});

test("unknown routes are 404", async () => {
  const res = await worker.fetch(new Request("https://t.example/"), { DB: fakeDb() });
  assert.equal(res.status, 404);
});
