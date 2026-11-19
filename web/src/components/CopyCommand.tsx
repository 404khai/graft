import { useEffect, useState } from "react";

export const BREW_INSTALL = "brew install 404khai/graft/graft";

export async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // Older browsers, or a page without clipboard permission.
    const area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    return ok;
  }
}

/** A command shown as a button; clicking it copies the command. */
export function CopyCommand({
  command,
  className = "btn btn-dark",
  prompt = true,
}: {
  command: string;
  className?: string;
  prompt?: boolean;
}) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1600);
    return () => clearTimeout(t);
  }, [copied]);

  return (
    <button
      type="button"
      className={`${className} copy-cmd`}
      onClick={async () => setCopied(await copyText(command))}
      aria-label={`Copy: ${command}`}
      title="Click to copy"
    >
      {prompt && <span className="prompt">$</span>}
      <span className="copy-text">{command}</span>
      <span className="copy-state" aria-live="polite">{copied ? "Copied" : ""}</span>
    </button>
  );
}
