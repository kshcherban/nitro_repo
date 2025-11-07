import { describe, expect, it } from "vitest";
import { themeTokens, applyThemeTokens } from "@/utils/themeTokens";

describe("theme tokens", () => {
  it("applies CSS custom properties to the target element", () => {
    const target = document.createElement("div");

    applyThemeTokens(target);

    for (const [variable, value] of Object.entries(themeTokens)) {
      expect(target.style.getPropertyValue(variable)).toBe(value);
    }
  });
});
