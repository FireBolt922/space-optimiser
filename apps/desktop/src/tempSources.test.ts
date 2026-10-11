import { describe, expect, it } from "vitest";
import { tempSource } from "./tempSources";

describe("tempSource", () => {
  it("puts Apple's background services together", () => {
    expect(tempSource("com.apple.ThreadCommissionerService")).toBe("system");
    expect(tempSource("com.apple.avconferenced")).toBe("system");
  });

  it("puts folders named after an app together", () => {
    expect(tempSource(".com.openai.codex.9XF9pN")).toBe("apps");
    expect(tempSource("org.mozilla.firefox.tmp")).toBe("apps");
  });

  it("recognises scratch names", () => {
    expect(tempSource(".tmpOJTzZZ")).toBe("unnamed");
    expect(tempSource("tmp.a1b2c3")).toBe("unnamed");
    expect(tempSource("TemporaryItems")).toBe("unnamed");
    expect(tempSource("3F2504E0-4F89-11D3-9A0C-0305E82C3301")).toBe("unnamed");
    expect(tempSource("a8Xk2pQ9")).toBe("unnamed");
  });

  it("leaves readable names as other", () => {
    expect(tempSource("old-download.bin")).toBe("other");
    expect(tempSource("<img src=x onerror=alert(1)>")).toBe("other");
    expect(tempSource("installer")).toBe("other");
  });
});
