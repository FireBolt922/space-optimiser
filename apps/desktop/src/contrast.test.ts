// @vitest-environment node
// WCAG 2.2 AA: every text colour pair the UI uses must reach 4.5:1 in both
// themes. Reads the real tokens file, so a token change that breaks contrast
// fails here.
import { describe, expect, it } from "vitest";

// Vitest replaces CSS imports (even `?raw`) with an empty string, so read the
// file directly. The specifier is a variable because the app has no Node types.
const FS = "node:fs";
const { readFileSync } = (await import(/* @vite-ignore */ FS)) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
const tokensCss = readFileSync(new URL("./styles/tokens.css", import.meta.url), "utf8");

type Tokens = Map<string, string>;

// Prettier puts each selector of a list on its own line; compare on one line.
const flatCss = tokensCss.replace(/,\s*\n\s*/g, ", ");

function block(selector: string): string {
  const start = flatCss.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no block for ${selector}`);
  return flatCss.slice(start, flatCss.indexOf("}", start));
}

function declarations(css: string): Tokens {
  const out: Tokens = new Map();
  for (const m of css.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out.set(m[1], m[2].trim());
  return out;
}

const light = declarations(block(':root, [data-theme="light"]'));
// Dark overrides the light block; primitives come from the light block.
const dark: Tokens = new Map([...light, ...declarations(block('[data-theme="dark"]'))]);

function resolve(tokens: Tokens, name: string): string {
  let value = tokens.get(name);
  for (let i = 0; value?.startsWith("var(") && i < 10; i++) {
    value = tokens.get(value.slice(4, -1).trim());
  }
  if (!value || !/^#[0-9a-f]{6}$/i.test(value)) throw new Error(`${name} is not a hex colour`);
  return value;
}

function luminance(hex: string): number {
  const channel = (i: number) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
}

function ratio(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

// [text, background] pairs used by app.css.
const PAIRS: [string, string][] = [
  ...["--ink", "--ink-muted"].flatMap((ink) =>
    ["--surface", "--surface-subtle", "--surface-raised"].map((bg): [string, string] => [ink, bg]),
  ),
  ["--ink", "--warning-subtle"],
  ["--ink", "--surface-sunken"],
  ["--on-primary", "--primary"],
  ["--on-primary", "--primary-hover"],
  ["--on-primary-subtle", "--primary-subtle"],
  ["--link", "--surface"],
  ["--link", "--warning-subtle"],
  ["--danger", "--surface"],
  ["--danger", "--surface-raised"],
];

describe.each([
  ["light", light],
  ["dark", dark],
])("%s theme contrast", (_theme, tokens) => {
  it.each(PAIRS)("%s on %s is at least 4.5:1", (ink, bg) => {
    expect(ratio(resolve(tokens, ink), resolve(tokens, bg))).toBeGreaterThanOrEqual(4.5);
  });
});

it("the ratio maths matches known values", () => {
  expect(ratio("#000000", "#ffffff")).toBeCloseTo(21, 1);
  expect(ratio("#777777", "#ffffff")).toBeCloseTo(4.48, 1);
});
