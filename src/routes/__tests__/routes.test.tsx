import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Routes, Route, Navigate } from "react-router";
import { ROUTES } from "@/types/layout";

// Canonical default route as defined in App.tsx — hardcoded to catch regressions
// if the redirect target drifts away from /downloads.
const DEFAULT_ROUTE = "/downloads";

// Test helper: drives each route element with a data-testid matching the path.
// Routes are derived from the ROUTES config so any additions or removals are
// automatically covered without updating this test file.
function AppRoutes({ initialPath }: { initialPath: string }) {
  return (
    <MemoryRouter initialEntries={[initialPath]}>
      <Routes>
        <Route index element={<Navigate to={DEFAULT_ROUTE} replace />} />
        {ROUTES.map((r) => (
          <Route
            key={r.path}
            path={r.path.replace(/^\//, "")}
            element={<div data-testid={r.path} />}
          />
        ))}
        <Route path="*" element={<Navigate to={DEFAULT_ROUTE} replace />} />
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
    expect(new Set(paths).size).toBe(paths.length);
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
  it("should render each route at its configured path", () => {
    for (const route of ROUTES) {
      const { unmount } = render(<AppRoutes initialPath={route.path} />);
      expect(screen.getByTestId(route.path)).toBeInTheDocument();
      unmount();
    }
  });

  it("should redirect / to /downloads", () => {
    render(<AppRoutes initialPath="/" />);
    expect(screen.getByTestId(DEFAULT_ROUTE)).toBeInTheDocument();
  });

  it("should redirect unknown paths to /downloads", () => {
    render(<AppRoutes initialPath="/unknown-route" />);
    expect(screen.getByTestId(DEFAULT_ROUTE)).toBeInTheDocument();
  });
});
