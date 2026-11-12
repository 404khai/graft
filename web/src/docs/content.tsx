import type { ReactNode } from "react";
import type { FloraKind } from "../components/Flora";

export const GH = "https://github.com/404khai/graft";

export function Code({ children, lang = "sh" }: { children: string; lang?: string }) {
  return (
    <pre className="doc-code" data-lang={lang}>
      <code>
        {children.split("\n").map((line, i) => {
          const comment = line.indexOf("  #");
          const [cmd, note] = comment >= 0 ? [line.slice(0, comment), line.slice(comment)] : [line, ""];
          return (
            <span key={i} className="doc-code-line">
              {lang === "sh" && cmd.trim() && <span className="prompt">$ </span>}
              {cmd}
              {note && <span className="doc-code-note">{note}</span>}
              {"\n"}
            </span>
          );
        })}
      </code>
    </pre>
  );
}

export type DocSection = { title: string; body: ReactNode };
export type DocPage = {
  id: string;
  title: string;
  group: "Get started" | "Concepts" | "Reference";
  summary: string;
  art: FloraKind;
  sections: DocSection[];
};

export const PAGES: DocPage[] = [
  {
    id: "quickstart",
    title: "Quickstart",
    group: "Get started",
    summary: "Install Graft, register a Root and create your first graft in a minute.",
    art: "tree",
    sections: [
      {
        title: "Install",
        body: (
          <>
            <p>
              Prebuilt binaries cover Linux x86_64 and aarch64, and macOS arm64. Download an archive
              from the <a href={`${GH}/releases`}>releases</a>, verify it, and put <code>graft</code>{" "}
              and <code>graftd</code> on your <code>PATH</code>. Each release also carries a Homebrew
              formula.
            </p>
            <Code>{`gh attestation verify <archive> --repo 404khai/graft`}</Code>
            <p>Or build from source:</p>
            <Code>{`cargo install --locked --git https://github.com/404khai/graft graft-cli graft-daemon`}</Code>
          </>
        ),
      },
      {
        title: "Check your machine",
        body: (
          <>
            <p>
              <code>graft doctor</code> reports the platform, the filesystem your repository is on,
              whether it supports reflinks (tested by cloning a scratch file), where{" "}
              <code>$GRAFT_HOME</code> is, and whether FUSE and NFS are available.
            </p>
            <Code>{`graft doctor`}</Code>
          </>
        ),
      },
      {
        title: "Your first graft",
        body: (
          <>
            <Code>{`graft init                # register this repository as a Root
graft new auth            # a graft checked out at HEAD
cd "$(graft path auth)"   # work in it like any git checkout
graft ls                  # name, base, state, changed files, unshared disk
graft status auth         # what changed since the base
graft diff auth           # as a unified diff
graft prune auth          # refuses if work would be lost; --force to discard`}</Code>
            <p>
              A graft is anonymous: it never takes a branch lock, so Root and every graft can have{" "}
              <code>main</code> checked out at once.
            </p>
          </>
        ),
      },
    ],
  },
  {
    id: "concepts",
    title: "Concepts",
    group: "Concepts",
    summary: "Root, grafts, deltas, layers and the op log: the words the rest of the docs use.",
    art: "graft",
    sections: [
      {
        title: "Root and grafts",
        body: (
          <dl className="doc-terms">
            <dt>Root</dt>
            <dd>The main repository: its git object store plus Graft's layer cache.</dd>
            <dt>Graft</dt>
            <dd>One agent workspace: a base snapshot, a private delta, a Blueprint and a sandbox policy, mounted as a directory.</dd>
            <dt>Delta</dt>
            <dd>The files a graft has changed relative to its base. A graft costs only this.</dd>
            <dt>Op log</dt>
            <dd>An append-only journal of every write, rename and delete in a graft.</dd>
            <dt>Layer</dt>
            <dd>A shared, read-only, content-addressed tree such as <code>node_modules</code>, built once per input hash.</dd>
          </dl>
        ),
      },
      {
        title: "Backends",
        body: (
          <>
            <p>
              A <b>reflink</b> graft is a real checkout whose unchanged files share disk blocks with
              Root (APFS, btrfs, XFS; a plain copy with one warning on ext4). A <b>virtual</b> graft
              writes only metadata and serves files lazily from Root's object store: FUSE on Linux,
              an NFS loopback server on macOS, with no kernel extension.
            </p>
            <Code>{`graft new api --backend fuse   # Linux
graft new api --backend nfs    # macOS`}</Code>
          </>
        ),
      },
      {
        title: "Bud and Ring",
        body: (
          <>
            <p>Virtual grafts fork and checkpoint as metadata operations.</p>
            <Code>{`graft bud auth auth-alt               # fork: same base, a copy of the delta
graft ring auth before-refactor       # checkpoint; graft ring auth --list
graft rewind auth before-refactor     # go back; prints how to undo it`}</Code>
          </>
        ),
      },
    ],
  },
  {
    id: "commands",
    title: "Commands",
    group: "Reference",
    summary: "Every graft command, what it does and the flags you'll reach for.",
    art: "branch",
    sections: [
      {
        title: "Workspaces",
        body: (
          <table className="doc-table">
            <tbody>
              {[
                ["graft init", "Register the current repository as a Root. Idempotent."],
                ["graft new <name> [--from <ref>]", "Create a graft. --backend, --blueprint."],
                ["graft ls", "Grafts with base, state, changed files and unshared disk."],
                ["graft path <name>", "Print a graft's directory."],
                ["graft status / diff <name>", "What changed since the base."],
                ["graft log <name>", "The graft's op log."],
                ["graft prune <name> [--force]", "Remove a graft; refuses to lose work."],
              ].map(([c, d]) => (
                <tr key={c}><td><code>{c}</code></td><td>{d}</td></tr>
              ))}
            </tbody>
          </table>
        ),
      },
      {
        title: "Running agents",
        body: (
          <table className="doc-table">
            <tbody>
              {[
                ["graft run <name> -- <cmd>", "Run a command confined to the graft."],
                ["graft env <name>", "Print the graft's resolved environment."],
                ["graft radar [--watch]", "Overlapping work across live grafts."],
                ["graft plan <task.md> [--apply]", "Split a task into grafts along the dependency graph."],
              ].map(([c, d]) => (
                <tr key={c}><td><code>{c}</code></td><td>{d}</td></tr>
              ))}
            </tbody>
          </table>
        ),
      },
      {
        title: "Landing work",
        body: (
          <table className="doc-table">
            <tbody>
              {[
                ["graft integrate <name...> [--dry-run]", "Land grafts into Root through the queue."],
                ["graft queue", "What landed, and why anything stopped."],
                ["graft rebase <name>", "Bring a live graft up to Root's head."],
                ["graft gc", "Remove layers no graft uses."],
                ["graft doctor", "What this machine supports."],
              ].map(([c, d]) => (
                <tr key={c}><td><code>{c}</code></td><td>{d}</td></tr>
              ))}
            </tbody>
          </table>
        ),
      },
    ],
  },
  {
    id: "blueprints",
    title: "Blueprints",
    group: "Concepts",
    summary: "Give every graft its own ports, database, volumes and .env with graft.toml.",
    art: "leaf1",
    sections: [
      {
        title: "A private runtime",
        body: (
          <>
            <p>
              A Blueprint in <code>graft.toml</code> gives every graft its own ports, Postgres
              database, Docker volumes and <code>.env</code> files, so two agents can run the same app
              and its tests side by side.
            </p>
            <Code lang="toml">{`[blueprint.web]
layers = ["node_modules"]
setup = ["npm run db:migrate"]
env_files = [".env.example"]

[blueprint.web.env]
DATABASE_URL = "postgres://localhost:{{port.db}}/{{graft.name}}"

[[blueprint.web.port]]
name = "app"
container_port = 3000`}</Code>
            <Code>{`graft new a --blueprint web
graft env a`}</Code>
          </>
        ),
      },
      {
        title: "Shared layers",
        body: (
          <>
            <p>
              Declare dependency trees as layers, approve the build command once with{" "}
              <code>graft trust</code>, and every graft after the first gets <code>node_modules</code>{" "}
              (or <code>target/</code>) as a reflink in milliseconds.
            </p>
            <Code lang="toml">{`[[layer]]
path = "node_modules"
inputs = ["package-lock.json"]
build = "npm ci"`}</Code>
          </>
        ),
      },
    ],
  },
  {
    id: "radar",
    title: "Radar",
    group: "Concepts",
    summary: "See overlapping edits between live grafts while agents work.",
    art: "leaf2",
    sections: [
      {
        title: "Three levels",
        body: (
          <>
            <p>
              <b>Path</b>: two grafts changed the same file. <b>Symbol</b>: the same function, type or
              export (tree-sitter: Rust, TypeScript, Python, Go). <b>Behaviour</b>: with a{" "}
              <code>[radar] test</code> command, Graft merges overlapping grafts in scratch checkouts and
              reports tests that fail only when combined.
            </p>
            <Code>{`graft radar           # overlaps by severity
graft radar --watch   # keep it current`}</Code>
          </>
        ),
      },
    ],
  },
  {
    id: "integrate",
    title: "Integrate",
    group: "Concepts",
    summary: "Land finished grafts in Root through a tested queue, as plain git history.",
    art: "leaf3",
    sections: [
      {
        title: "The queue",
        body: (
          <>
            <p>
              <code>graft integrate</code> merges each graft onto the target's head in a scratch
              checkout (a line merge, then <a href="https://mergiraf.org">Mergiraf</a> when it is
              installed), runs the <code>[integrate] test</code> gate, and commits only if it passes.
              A failed gate returns the graft to Live with the output attached.
            </p>
            <Code>{`graft integrate auth api
graft queue
graft rebase billing`}</Code>
          </>
        ),
      },
    ],
  },
  {
    id: "mcp",
    title: "MCP server",
    group: "Reference",
    summary: "Let Claude Code, Codex, Cursor and other agents drive Graft directly.",
    art: "graft",
    sections: [
      {
        title: "Serve Graft to agents",
        body: (
          <>
            <p>
              <code>graft mcp</code> serves Graft to any MCP client over stdio: agents create grafts,
              run commands in them, watch Radar and integrate. Destructive calls need{" "}
              <code>confirm: true</code>. Register the Root once with <code>graft init</code>, then
              point your client at it.
            </p>
          </>
        ),
      },
      {
        title: "Clients",
        body: (
          <>
            <p><b>Claude Code</b></p>
            <Code>{`claude mcp add graft -- graft mcp --root /path/to/repo`}</Code>
            <p><b>Codex</b>, in <code>~/.codex/config.toml</code></p>
            <Code lang="toml">{`[mcp_servers.graft]
command = "graft"
args = ["mcp", "--root", "/path/to/repo"]`}</Code>
            <p><b>Cursor</b>, in <code>.cursor/mcp.json</code></p>
            <Code lang="json">{`{ "mcpServers": { "graft": { "command": "graft", "args": ["mcp", "--root", "/path/to/repo"] } } }`}</Code>
          </>
        ),
      },
    ],
  },
];
