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

export function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
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
