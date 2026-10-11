// File names are attacker-controlled (anyone can name a file in a temp
// folder), so the UI renders them as text only (phase 7).
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/", "src-tauri/", "coverage/"] },
  {
    files: ["src/**/*.{ts,tsx}"],
    extends: [tseslint.configs.recommended],
    rules: {
      "no-restricted-syntax": [
        "error",
        {
          selector: "JSXAttribute[name.name='dangerouslySetInnerHTML']",
          message: "Render text only. File names can contain markup.",
        },
        {
          selector: "JSXAttribute[name.name='style']",
          message: "No inline styles: the release CSP blocks them. Use a class.",
        },
        {
          selector: "MemberExpression[property.name='innerHTML']",
          message: "Render text only. File names can contain markup.",
        },
      ],
    },
  },
);
