import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PluginsToolbar } from "../PluginsToolbar";

const CATEGORIES = ["all", "crawler", "hoster", "captcha"];

describe("PluginsToolbar", () => {
  it("renders a pill for every provided category", () => {
    render(
      <PluginsToolbar
        categories={CATEGORIES}
        activeCategory="all"
        onCategoryChange={vi.fn()}
        search=""
        onSearchChange={vi.fn()}
      />,
    );
    expect(screen.getAllByRole("tab")).toHaveLength(CATEGORIES.length);
  });

  it("marks the active category pill with aria-selected", () => {
    render(
      <PluginsToolbar
        categories={CATEGORIES}
        activeCategory="crawler"
        onCategoryChange={vi.fn()}
        search=""
        onSearchChange={vi.fn()}
      />,
    );
    const active = screen.getByRole("tab", { selected: true });
    expect(active).toHaveTextContent(/crawler/i);
  });

  it("calls onCategoryChange when a pill is clicked", async () => {
    const user = userEvent.setup();
    const onCategoryChange = vi.fn();
    render(
      <PluginsToolbar
        categories={CATEGORIES}
        activeCategory="all"
        onCategoryChange={onCategoryChange}
        search=""
        onSearchChange={vi.fn()}
      />,
    );
    await user.click(screen.getByRole("tab", { name: /crawler/i }));
    expect(onCategoryChange).toHaveBeenCalledWith("crawler");
  });

  it("emits onSearchChange as the user types", async () => {
    const user = userEvent.setup();
    const onSearchChange = vi.fn();
    render(
      <PluginsToolbar
        categories={CATEGORIES}
        activeCategory="all"
        onCategoryChange={vi.fn()}
        search=""
        onSearchChange={onSearchChange}
      />,
    );
    await user.type(screen.getByRole("searchbox"), "y");
    expect(onSearchChange).toHaveBeenCalledWith("y");
  });
});
