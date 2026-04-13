import { describe, it, expect } from "vitest";
import i18n from "./i18n";

function waitForInit(): Promise<void> {
  if (i18n.isInitialized) return Promise.resolve();
  return new Promise((resolve) => {
    i18n.on("initialized", () => resolve());
  });
}

describe("i18n html lang attribute", () => {
  it("should set document lang to the resolved language after init", async () => {
    await waitForInit();
    expect(document.documentElement.lang).toBe(i18n.language);
  });

  it("should update document lang when language changes to fr", async () => {
    await i18n.changeLanguage("fr");
    expect(document.documentElement.lang).toBe("fr");
  });

  it("should update document lang when language changes back to en", async () => {
    await i18n.changeLanguage("en");
    expect(document.documentElement.lang).toBe("en");
  });
});
