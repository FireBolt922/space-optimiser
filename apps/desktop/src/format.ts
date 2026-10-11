// Sizes use decimal units, as macOS Finder and the Ubuntu file manager do.
const UNITS = ["bytes", "KB", "MB", "GB", "TB"];

export function formatBytes(bytes: number): string {
  if (bytes < 1000) return `${bytes} bytes`;
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)} ${UNITS[unit]}`;
}

const COUNT = new Intl.NumberFormat("en-US");

/** 2203944 → "2,203,944". */
export function formatCount(count: number): string {
  return COUNT.format(count);
}

export function plural(count: number, one: string, many: string): string {
  return `${formatCount(count)} ${count === 1 ? one : many}`;
}

/** Whole seconds in words: "12 seconds", "1 minute 12 seconds", "1 hour 3 minutes". */
export function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000);
  if (total < 1) return "less than a second";
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  if (hours > 0) {
    return minutes > 0
      ? `${plural(hours, "hour", "hours")} ${plural(minutes, "minute", "minutes")}`
      : plural(hours, "hour", "hours");
  }
  if (minutes > 0) {
    return seconds > 0
      ? `${plural(minutes, "minute", "minutes")} ${plural(seconds, "second", "seconds")}`
      : plural(minutes, "minute", "minutes");
  }
  return plural(seconds, "second", "seconds");
}

const RISK_WORDS: Record<string, string> = {
  low: "Low",
  medium: "Medium",
  high: "High",
  info: "Info",
};

export function riskWord(risk: string): string {
  return RISK_WORDS[risk] ?? risk;
}
