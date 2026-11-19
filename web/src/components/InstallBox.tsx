import { Icon } from "./Icon";
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
          <Icon name="check" />
        ) : (
          <Icon name="copy" />
        )}
      </button>
      <span className="sr-only" aria-live="polite">{copied ? "Copied to clipboard" : ""}</span>
    </div>
  );
}
