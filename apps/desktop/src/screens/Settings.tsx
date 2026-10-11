import type { ThemeChoice } from "../theme";

interface Props {
  theme: ThemeChoice;
  onTheme: (choice: ThemeChoice) => void;
}

const CHOICES: { value: ThemeChoice; label: string; hint?: string }[] = [
  { value: "dark", label: "Dark", hint: "Default" },
  { value: "light", label: "Light" },
  { value: "system", label: "Match system" },
];

// Settings has no primary action: a choice applies as soon as it is made.
export function Settings({ theme, onTheme }: Props) {
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
    </section>
  );
}
