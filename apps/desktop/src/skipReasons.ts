// Plain words for why the scan left something alone, keyed by the engine's
// skip reason. A reason missing here falls back to the engine's own text.

export const SKIP_REASONS: Record<string, { title: string; explanation: string }> = {
  fresh: {
    title: "Changed in the last day",
    explanation: "An app may still be using these. They are offered once they are a day old.",
  },
  project: {
    title: "Looks like a project",
    explanation:
      "They contain files such as .git or package.json, so they may be your own work. Space Optimiser never cleans them.",
  },
  socket_or_pipe: {
    title: "In use by a running app",
    explanation: "They hold a live connection, such as an editor or an ssh agent.",
  },
  locked: {
    title: "Locked",
    explanation: "Read-only or locked by the system. Space Optimiser never unlocks files.",
  },
  unreadable: {
    title: "Could not be read",
    explanation: "Space Optimiser could not look inside, so it does not judge them.",
  },
  other_owner: {
    title: "Belongs to another user",
    explanation: "Only your own files are offered.",
  },
  link: {
    title: "Links",
    explanation: "Links are never followed, so what they point to stays as it is.",
  },
  special_file: { title: "Device files", explanation: "These are not ordinary files." },
  protected_folder: {
    title: "Protected folders",
    explanation: "They are or contain a folder Space Optimiser never touches, such as Documents.",
  },
  other_filesystem: {
    title: "On another disk",
    explanation: "Part of them is on another disk, so they are not judged here.",
  },
  outside_home: {
    title: "Outside your home folder",
    explanation: "The folder's real location is not inside your home folder.",
  },
  not_a_log: {
    title: "Not only logs",
    explanation: "These log folders also hold files that are not logs.",
  },
};
