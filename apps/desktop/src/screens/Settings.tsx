import type { ComponentProps } from "react";
import type { ThemeChoice } from "../theme";
import { UpdatesSection } from "../components/UpdatesSection";

interface Props {
  theme: ThemeChoice;
  onTheme: (choice: ThemeChoice) => void;
  updates: ComponentProps<typeof UpdatesSection>;
}

const CHOICES: { value: ThemeChoice; label: string; hint?: string }[] = [
  { value: "dark", label: "Dark", hint: "Default" },
  { value: "light", label: "Light" },
  { value: "system", label: "Match system" },
];

// A choice applies as soon as it is made. The only primary button is the
// update action, shown once a check found a new version.
export function Settings({ theme, onTheme, updates }: Props) {
  return (
    <section className="screen" aria-labelledby="settings-title">
      <h1 id="settings-title" className="title-xl" tabIndex={-1}>
        Settings
      </h1>
      <fieldset className="card choice-group">
        <legend className="title">Theme</legend>
        {CHOICES.map(({ value, label, hint }) => (
          <label key={value} className="choice">
            <input
              type="radio"
              name="theme"
              value={value}
              checked={theme === value}
              onChange={() => onTheme(value)}
            />
            <span>
              {label}
              {hint && <span className="ink-muted"> ({hint})</span>}
            </span>
          </label>
        ))}
        <p className="body-sm ink-muted">
          Match system follows the light or dark setting of your computer.
        </p>
      </fieldset>
      <UpdatesSection {...updates} />
    </section>
  );
}
