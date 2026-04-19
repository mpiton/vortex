import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PluginsHeader } from "../PluginsHeader";

describe("PluginsHeader", () => {
  it("renders the 'Plugins' title", () => {
    render(
      <PluginsHeader enabledCount={0} disabledCount={0} onRefresh={vi.fn()} isRefreshing={false} />,
    );
    expect(screen.getByText(/plugins/i)).toBeInTheDocument();
  });

  it("shows the enabled and disabled counters", () => {
    render(
      <PluginsHeader enabledCount={4} disabledCount={2} onRefresh={vi.fn()} isRefreshing={false} />,
    );
    expect(screen.getByTestId("plugins-enabled-count")).toHaveTextContent("4");
    expect(screen.getByTestId("plugins-disabled-count")).toHaveTextContent("2");
  });

  it("invokes onRefresh when the refresh button is clicked", async () => {
    const user = userEvent.setup();
    const onRefresh = vi.fn();
    render(
      <PluginsHeader
        enabledCount={0}
        disabledCount={0}
        onRefresh={onRefresh}
        isRefreshing={false}
      />,
    );
    await user.click(screen.getByRole("button", { name: /(check updates|vérifier)/i }));
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("disables the refresh button while refreshing", () => {
    render(<PluginsHeader enabledCount={0} disabledCount={0} onRefresh={vi.fn()} isRefreshing />);
    const button = screen.getByRole("button", {
      name: /(check updates|vérifier)/i,
    });
    expect(button).toBeDisabled();
  });
});
