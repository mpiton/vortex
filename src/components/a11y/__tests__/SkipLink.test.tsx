import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { SkipLink } from "../SkipLink";

describe("SkipLink", () => {
  it("should render an anchor pointing to #main-content", () => {
    render(<SkipLink />);
    const link = screen.getByRole("link");
    expect(link).toHaveAttribute("href", "#main-content");
  });

  it("should display the translated skip-to-main label", () => {
    render(<SkipLink />);
    expect(screen.getByRole("link")).toHaveTextContent("Skip to main content");
  });

  it("should be visually hidden by default via sr-only", () => {
    render(<SkipLink />);
    const link = screen.getByRole("link");
    expect(link.className).toMatch(/\bsr-only\b/);
  });

  it("should reveal itself when focused via focus:not-sr-only", () => {
    render(<SkipLink />);
    const link = screen.getByRole("link");
    expect(link.className).toMatch(/focus:not-sr-only/);
  });

  it("should focus the #main-content target when activated", () => {
    const target = document.createElement("div");
    target.id = "main-content";
    target.tabIndex = -1;
    document.body.appendChild(target);

    try {
      render(<SkipLink />);
      const link = screen.getByRole("link");
      fireEvent.click(link);
      expect(document.activeElement).toBe(target);
    } finally {
      document.body.removeChild(target);
    }
  });

  it("should not navigate (no hash change) when activated", () => {
    const target = document.createElement("div");
    target.id = "main-content";
    target.tabIndex = -1;
    document.body.appendChild(target);
    const previousHash = window.location.hash;
    window.history.replaceState(null, "", "#before-skip-link");

    try {
      render(<SkipLink />);
      fireEvent.click(screen.getByRole("link"));
      expect(window.location.hash).toBe("#before-skip-link");
    } finally {
      window.history.replaceState(null, "", previousHash || window.location.pathname);
      document.body.removeChild(target);
    }
  });
});
