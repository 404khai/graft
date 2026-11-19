import { HugeiconsIcon } from "@hugeicons/react";
import {
  Alert02Icon,
  ArrowDataTransferHorizontalIcon,
  ArrowLeft02Icon,
  ArrowRight02Icon,
  ArrowTurnBackwardIcon,
  ArrowUpDownIcon,
  ArrowUpRight01Icon,
  Copy01Icon,
  Search01Icon,
  StarIcon,
  Tick02Icon,
} from "@hugeicons/core-free-icons";

/** Hugeicons (free set) used across the site. One name per meaning. */
const ICONS = {
  alert: Alert02Icon,
  "arrow-right": ArrowRight02Icon,
  "arrow-left": ArrowLeft02Icon,
  external: ArrowUpRight01Icon,
  "both-ways": ArrowDataTransferHorizontalIcon,
  "up-down": ArrowUpDownIcon,
  enter: ArrowTurnBackwardIcon,
  copy: Copy01Icon,
  check: Tick02Icon,
  search: Search01Icon,
  star: StarIcon,
} as const;

export type IconName = keyof typeof ICONS;

export function Icon({ name, size = 16, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <HugeiconsIcon
      icon={ICONS[name]}
      size={size}
      strokeWidth={1.8}
      className={`icon${className ? ` ${className}` : ""}`}
      aria-hidden="true"
    />
  );
}
