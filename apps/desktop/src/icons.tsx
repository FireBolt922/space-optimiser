// Simple line icons drawn for this app. They are decoration next to a visible
// text label, so they are hidden from assistive technology.
import type { ReactNode } from "react";

function Icon({ children }: { children: ReactNode }) {
  return (
    <svg
      className="icon"
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

/** A drive with a sweep line: the clean-up view. */
export function ScanIcon() {
  return (
    <Icon>
      <rect x="3.5" y="5" width="17" height="14" rx="2" />
      <path d="M3.5 14h17" />
      <path d="M7 16.5h3" />
      <path d="M8 9.5h8" />
    </Icon>
  );
}

/** Three sliders. */
export function SettingsIcon() {
  return (
    <Icon>
      <path d="M4 7h9M17 7h3" />
      <circle cx="15" cy="7" r="2" />
      <path d="M4 12h3M11 12h9" />
      <circle cx="9" cy="12" r="2" />
      <path d="M4 17h11M19 17h1" />
      <circle cx="17" cy="17" r="2" />
    </Icon>
  );
}

export function HelpIcon() {
  return (
    <Icon>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M9.75 9.75a2.25 2.25 0 1 1 3.1 2.08c-.5.2-.85.68-.85 1.22v.45" />
      <path d="M12 16.75v.01" />
    </Icon>
  );
}

export function InfoIcon() {
  return (
    <Icon>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 11v5.5" />
      <path d="M12 7.75v.01" />
    </Icon>
  );
}

export function FolderIcon() {
  return (
    <Icon>
      <path d="M3.5 7.5a1.5 1.5 0 0 1 1.5-1.5h4l2 2h8a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 1-1.5 1.5H5a1.5 1.5 0 0 1-1.5-1.5z" />
    </Icon>
  );
}

/** An arrow down into a tray: save a file. */
export function ExportIcon() {
  return (
    <Icon>
      <path d="M12 4.5v10" />
      <path d="M8 10.5l4 4 4-4" />
      <path d="M4.5 15.5v2.5a1.5 1.5 0 0 0 1.5 1.5h12a1.5 1.5 0 0 0 1.5-1.5v-2.5" />
    </Icon>
  );
}
