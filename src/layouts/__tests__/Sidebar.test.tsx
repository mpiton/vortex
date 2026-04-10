import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { Sidebar } from "../Sidebar";

function renderSidebar(initialRoute = "/downloads") {
  return render(
    <MemoryRouter initialEntries={[initialRoute]}>
      <Sidebar />
    </MemoryRouter>,
  );
}

describe("Sidebar", () => {
  it("should render all 10 navigation items", () => {
    renderSidebar();
    const labels = [
      "Downloads",
      "Link Grabber",
      "Packages",
      "Accounts",
      "Captcha",
      "Plugins",
      "Scheduler",
      "History",
      "Statistics",
      "Settings",
    ];
    for (const label of labels) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
  });

  it("should render the Vortex title", () => {
    renderSidebar();
    expect(screen.getByText("Vortex")).toBeInTheDocument();
  });

  it("should highlight active route with correct classes", () => {
    renderSidebar("/settings");
    const settingsLink = screen.getByText("Settings").closest("a");
    expect(settingsLink).toHaveClass("bg-indigo-600");
    expect(settingsLink).toHaveClass("font-semibold");
  });

  it("should not highlight inactive routes", () => {
    renderSidebar("/downloads");
    const settingsLink = screen.getByText("Settings").closest("a");
    expect(settingsLink).not.toHaveClass("bg-indigo-600");
  });

  it("should render navigation links with correct paths", () => {
    renderSidebar();
    const downloadsLink = screen.getByText("Downloads").closest("a");
    expect(downloadsLink).toHaveAttribute("href", "/downloads");
    const settingsLink = screen.getByText("Settings").closest("a");
    expect(settingsLink).toHaveAttribute("href", "/settings");
  });
});
