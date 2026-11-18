import { useEffect, useState } from "react";
import { copyText } from "./CopyCommand";

/** Install methods from the README. */
const METHODS = [
  { id: "brew", label: "brew", command: "brew install 404khai/graft/graft" },
  {
    id: "cargo",
    label: "cargo",
    command: "cargo install --locked --git https://github.com/404khai/graft graft-cli graft-daemon",
  },
] as const;

/** Install command with a method switch and a copy button. `suffix` is appended to every command. */
export function InstallBox({ suffix = "", className }: { suffix?: string; className?: string }) {
  const [method, setMethod] = useState<(typeof METHODS)[number]["id"]>("brew");
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1600);
    return () => clearTimeout(t);
  }, [copied]);
  const command = METHODS.find((m) => m.id === method)!.command + suffix;

  return (
    <div className={`install${className ? ` ${className}` : ""}`}>
      <div className="install-tabs" role="tablist" aria-label="Install with">
        {METHODS.map((m) => (
          <button
            key={m.id}
            type="button"
            role="tab"
            aria-selected={m.id === method}
            onClick={() => setMethod(m.id)}
          >
            {m.label}
          </button>
        ))}
      </div>
      <code className="install-cmd" title={command}>
        <span className="install-prompt">$</span> {command}
      </code>
      <button
        type="button"
        className={`install-copy${copied ? " is-copied" : ""}`}
        onClick={async () => setCopied(await copyText(command))}
        aria-label={copied ? "Copied" : `Copy: ${command}`}
        title={copied ? "Copied" : "Copy"}
      >
        {copied ? (
          <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m4.5 10.5 3.5 3.5 7.5-8" /></svg>
        ) : (
          <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="7" y="7" width="9" height="9" rx="2" /><path d="M13 7V5.5A1.5 1.5 0 0 0 11.5 4h-6A1.5 1.5 0 0 0 4 5.5v6A1.5 1.5 0 0 0 5.5 13H7" /></svg>
        )}
      </button>
      <span className="sr-only" aria-live="polite">{copied ? "Copied to clipboard" : ""}</span>
    </div>
  );
}
