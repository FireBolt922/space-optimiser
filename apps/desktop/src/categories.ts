// Result categories: a few headings a person can filter by. They only group
// catalog ids for display; the catalog in the engine still decides what is
// offered, and the UI still sends ids only.
import type { Candidate } from "./engine";

export type CategoryId = "temporary" | "browser" | "developer" | "system" | "logs" | "large";

export interface Category {
  id: CategoryId;
  title: string;
  /** One line under the heading, in the order-of-object, consequence style. */
  blurb: string;
}

export const CATEGORIES: Category[] = [
  { id: "temporary", title: "Temporary files", blurb: "Apps recreate these as they run." },
  { id: "browser", title: "Browsers", blurb: "Pages load from the network again." },
  {
    id: "developer",
    title: "Developer tools",
    blurb: "Package and build caches. The next install or build downloads again.",
  },
  { id: "system", title: "System caches", blurb: "The system rebuilds these when needed." },
  { id: "logs", title: "Logs", blurb: "Old log files that nothing reads any more." },
  { id: "large", title: "Large files", blurb: "Shown so you can decide. Never removed." },
];

const BY_CATALOG_ID: Record<string, CategoryId> = {
  "user-temp": "temporary",
  "browser-http-cache": "browser",
  "thumbnail-cache": "system",
  "windows-thumbcache": "system",
  "font-cache": "system",
  "user-logs-old": "logs",
  "docker-build-cache": "developer",
  "large-files-report": "large",
};

/** A catalog id the UI has not heard of lands in the closest general heading. */
export function categoryOf(catalogId: string): CategoryId {
  const known = BY_CATALOG_ID[catalogId];
  if (known) return known;
  if (catalogId.startsWith("pkg-")) return "developer";
  return "temporary";
}

/** Bytes a category's rows would free. Tool rows count their upper bound. */
export function categoryBytes(rows: Candidate[]): number {
  return rows.reduce((sum, c) => sum + (c.upper_bound_bytes ?? c.estimated_bytes), 0);
}
