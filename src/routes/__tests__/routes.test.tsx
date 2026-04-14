import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Routes, Route, Navigate } from "react-router";
import { ROUTES } from "@/types/layout";

// Minimal routing setup mirroring App.tsx — no layout wrapper needed here
function AppRoutes({ initialPath }: { initialPath: string }) {
  return (
    <MemoryRouter initialEntries={[initialPath]}>
      <Routes>
        <Route index element={<Navigate to="/downloads" replace />} />
        <Route path="downloads" element={<div>Downloads</div>} />
        <Route path="link-grabber" element={<div>Link Grabber</div>} />
        <Route path="packages" element={<div>Packages</div>} />
        <Route path="accounts" element={<div>Accounts</div>} />
        <Route path="captcha" element={<div>Captcha</div>} />
        <Route path="plugins" element={<div>Plugins</div>} />
        <Route path="scheduler" element={<div>Scheduler</div>} />
        <Route path="history" element={<div>History</div>} />
        <Route path="statistics" element={<div>Statistics</div>} />
        <Route path="settings" element={<div>Settings</div>} />
        <Route path="*" element={<Navigate to="/downloads" replace />} />
      </Routes>
    </MemoryRouter>
  );
}

describe("ROUTES config", () => {
  it("should define exactly 10 routes", () => {
    expect(ROUTES).toHaveLength(10);
  });

  it("should have unique paths", () => {
    const paths = ROUTES.map((r) => r.path);
    const unique = new Set(paths);
    expect(unique.size).toBe(paths.length);
  });

  it("should have all required fields on each route", () => {
    for (const route of ROUTES) {
      expect(route.icon).toBeDefined();
      expect(route.labelKey).toBeTruthy();
      expect(route.path).toMatch(/^\//);
      expect(route.shortcut).toBeTruthy();
    }
  });
});

describe("App routing", () => {
  it("should render downloads at /downloads", () => {
    render(<AppRoutes initialPath="/downloads" />);
    expect(screen.getByText("Downloads")).toBeInTheDocument();
  });

  it("should render link-grabber at /link-grabber", () => {
    render(<AppRoutes initialPath="/link-grabber" />);
    expect(screen.getByText("Link Grabber")).toBeInTheDocument();
  });

  it("should render all 10 routes", () => {
    const routePaths = [
      { path: "/downloads", label: "Downloads" },
      { path: "/link-grabber", label: "Link Grabber" },
      { path: "/packages", label: "Packages" },
      { path: "/accounts", label: "Accounts" },
      { path: "/captcha", label: "Captcha" },
      { path: "/plugins", label: "Plugins" },
      { path: "/scheduler", label: "Scheduler" },
      { path: "/history", label: "History" },
      { path: "/statistics", label: "Statistics" },
      { path: "/settings", label: "Settings" },
    ];

    for (const { path, label } of routePaths) {
      const { unmount } = render(<AppRoutes initialPath={path} />);
      expect(screen.getByText(label)).toBeInTheDocument();
      unmount();
    }
  });

  it("should redirect / to /downloads", () => {
    render(<AppRoutes initialPath="/" />);
    expect(screen.getByText("Downloads")).toBeInTheDocument();
  });

  it("should redirect unknown paths to /downloads", () => {
    render(<AppRoutes initialPath="/unknown-route" />);
    expect(screen.getByText("Downloads")).toBeInTheDocument();
  });
});
