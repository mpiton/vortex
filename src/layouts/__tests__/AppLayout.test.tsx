import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { MemoryRouter, Routes, Route } from "react-router";
import { AppLayout } from "../AppLayout";

const mockNavigate = vi.fn();

vi.mock("react-router", async () => {
  const actual = await vi.importActual("react-router");
  return {
    ...actual,
    useNavigate: () => mockNavigate,
  };
});

function renderAppLayout(initialRoute = "/downloads") {
  return render(
    <MemoryRouter initialEntries={[initialRoute]}>
      <Routes>
        <Route element={<AppLayout />}>
          <Route path="downloads" element={<div>Downloads Page</div>} />
          <Route path="settings" element={<div>Settings Page</div>} />
        </Route>
      </Routes>
    </MemoryRouter>,
  );
}

describe("AppLayout", () => {
  beforeEach(() => {
    mockNavigate.mockClear();
  });

  it("should render Sidebar, main content, and StatusBar", () => {
    renderAppLayout();
    expect(screen.getByText("Vx")).toBeInTheDocument();
    expect(screen.getByText("Downloads Page")).toBeInTheDocument();
    expect(screen.getByText(/vortex v0\.1\.0/)).toBeInTheDocument();
  });

  it("should navigate on modifier+1 keyboard shortcut", () => {
    renderAppLayout();
    const isMac = navigator.platform.includes("Mac");
    fireEvent.keyDown(window, { key: "1", ctrlKey: !isMac, metaKey: isMac });
    expect(mockNavigate).toHaveBeenCalledWith("/downloads");
  });

  it("should navigate to settings on modifier+,", () => {
    renderAppLayout();
    const isMac = navigator.platform.includes("Mac");
    fireEvent.keyDown(window, { key: ",", ctrlKey: !isMac, metaKey: isMac });
    expect(mockNavigate).toHaveBeenCalledWith("/settings");
  });

  it("should ignore keydown without Ctrl", () => {
    renderAppLayout();
    fireEvent.keyDown(window, { key: "1" });
    expect(mockNavigate).not.toHaveBeenCalled();
  });
});
