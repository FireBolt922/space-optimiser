import type { ComponentType } from "react";
import appIcon from "../../src-tauri/icons/source/space-optimiser-app-icon.svg";
import { HelpIcon, InfoIcon, ScanIcon, SettingsIcon } from "../icons";

export type View = "cleanup" | "settings" | "help" | "about";

const ITEMS: { view: View; label: string; Icon: ComponentType }[] = [
  { view: "cleanup", label: "Clean up", Icon: ScanIcon },
  { view: "settings", label: "Settings", Icon: SettingsIcon },
  { view: "help", label: "Help", Icon: HelpIcon },
  { view: "about", label: "About", Icon: InfoIcon },
];

interface Props {
  view: View;
  version: string | null;
  onNavigate: (view: View) => void;
}

export function Sidebar({ view, version, onNavigate }: Props) {
  return (
    <aside className="sidebar">
      <div className="brand">
        <img className="app-icon" src={appIcon} alt="" width={32} height={32} />
        <span className="product-name">Space Optimiser</span>
      </div>
      <nav aria-label="Main" className="nav">
        <ul className="nav-list">
          {ITEMS.map(({ view: target, label, Icon }) => (
            <li key={target}>
              <button
                type="button"
                className="nav-item"
                aria-current={view === target ? "page" : undefined}
                onClick={() => onNavigate(target)}
              >
                <Icon />
                {label}
              </button>
            </li>
          ))}
        </ul>
      </nav>
      {version && <p className="caption ink-muted sidebar-version">Version {version}</p>}
    </aside>
  );
}
