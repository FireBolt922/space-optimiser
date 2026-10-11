// Display-only sub-groups for Temporary files, read from the entry's name.
// Like categories.ts, this never changes what is offered or selected: the
// engine decides that, and the UI still sends ids.

export type TempSourceId = "apps" | "system" | "unnamed" | "other";

export const TEMP_SOURCES: { id: TempSourceId; title: string; description: string }[] = [
  {
    id: "apps",
    title: "App temporary folders",
    description: "Named after the app that left them, such as com.company.app.",
  },
  {
    id: "system",
    title: "macOS system services",
    description: "Left by macOS background services. macOS recreates what it still needs.",
  },
  {
    id: "unnamed",
    title: "Unnamed temporary files",
    description: "Random names that apps and installers use for scratch work.",
  },
  { id: "other", title: "Other", description: "Other files and folders more than a day old." },
];

const APPLE = /^com\.apple\./i;
/** A reverse-DNS app id: com.company.app, org.project.tool, ... */
const APP_ID = /^(com|org|net|io|dev|app|co)\.[^.\s]+\.[^.\s]/i;
const SCRATCH = /^(tmp|temp)/i;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
/** A short random token: letters and digits mixed, no separators. */
const TOKEN = /^(?=.*\d)(?=.*[a-z])[a-z0-9_]{6,16}$/i;

export function tempSource(name: string): TempSourceId {
  const bare = name.replace(/^\./, "");
  if (APPLE.test(bare)) return "system";
  if (APP_ID.test(bare)) return "apps";
  if (SCRATCH.test(bare) || UUID.test(bare) || TOKEN.test(bare)) return "unnamed";
  return "other";
}
