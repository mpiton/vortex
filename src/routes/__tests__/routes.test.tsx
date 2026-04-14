import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Routes, Route, Navigate } from "react-router";
import { ROUTES } from "@/types/layout";

// Test helper: drives each route element with a data-testid matching the path.
// Routes are derived from the ROUTES config so any additions or removals are
// automatically covered without updating this test file.
function AppRoutes({ initialPath }: { initialPath: string }) {
  return (
    <MemoryRouter initialEntries={[initialPath]}>
      <Routes>
        <Route index element={<Navigate to={ROUTES[0].path} replace />} />
        {ROUTES.map((r) => (
          <Route
            key={r.path}
            path={r.path.replace(/^\//, "")}
            element={<div data-testid={r.path} />}
          />
        ))}
        <Route path="*" element={<Navigate to={ROUTES[0].path} replace />} />
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

  it("should redirect / to the first route", () => {
    render(<AppRoutes initialPath="/" />);
    expect(screen.getByTestId(ROUTES[0].path)).toBeInTheDocument();
  });

  it("should redirect unknown paths to the first route", () => {
    render(<AppRoutes initialPath="/unknown-route" />);
    expect(screen.getByTestId(ROUTES[0].path)).toBeInTheDocument();
  });
});
