import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PluginStoreRow } from "../PluginStoreRow";
import type { PluginStoreEntry } from "@/types/plugin-store";

const baseEntry: PluginStoreEntry = {
  name: "vortex-mod-youtube",
  description: "YouTube downloader",
  author: "vortex-community",
  version: "1.1.2",
  installedVersion: "1.1.2",
  category: "crawler",
  official: true,
  status: "installed",
};

function renderRow(
  override: Partial<PluginStoreEntry> = {},
  handlers: Record<string, unknown> = {},
) {
  const defaultHandlers = {
    onInstall: vi.fn(),
    onUpdate: vi.fn(),
    onDisable: vi.fn(),
    onEnable: vi.fn(),
    onUninstall: vi.fn(),
    isInstalling: false,
    isUpdating: false,
    isLocallyDisabled: false,
  };
  const merged = { ...defaultHandlers, ...handlers };
  render(<PluginStoreRow entry={{ ...baseEntry, ...override }} {...merged} />);
  return merged;
}

describe("PluginStoreRow", () => {
  it("renders the plugin name, description and category", () => {
    renderRow();
    expect(screen.getByText("vortex-mod-youtube")).toBeInTheDocument();
    expect(screen.getByText(/YouTube downloader/)).toBeInTheDocument();
  });

  it("renders the category label translated through i18n", () => {
    renderRow({ category: "crawler" });
    expect(screen.getByText(/Crawlers/)).toBeInTheDocument();
  });

  it("falls back to the raw category slug when no translation exists", () => {
    renderRow({ category: "custom-unknown" });
    expect(screen.getByText(/custom-unknown/)).toBeInTheDocument();
  });

  it("renders a monogram icon derived from the plugin name", () => {
    renderRow({ name: "vortex-mod-youtube" });
    const icon = screen.getByTestId("plugin-icon");
    expect(icon).toHaveTextContent("YO");
  });

  it("strips the vortex-mod- prefix when generating the monogram", () => {
    renderRow({ name: "vortex-mod-soundcloud" });
    expect(screen.getByTestId("plugin-icon")).toHaveTextContent("SO");
  });

  it("shows an install button when the plugin is not installed", async () => {
    const user = userEvent.setup();
    const handlers = renderRow({ status: "not_installed", installedVersion: null });
    const button = screen.getByRole("button", { name: /install(er)?/i });
    await user.click(button);
    expect(handlers.onInstall).toHaveBeenCalledWith("vortex-mod-youtube");
  });

  it("does not render an install button when the plugin is already installed", () => {
    renderRow({ status: "installed" });
    expect(screen.queryByRole("button", { name: /^install(er)?$/i })).not.toBeInTheDocument();
  });

  it("renders an update pill prefixed with v when an update is available", async () => {
    const user = userEvent.setup();
    const handlers = renderRow({
      status: "update_available",
      version: "1.2.0",
      installedVersion: "1.1.0",
    });
    const pill = screen.getByRole("button", { name: /v1\.2\.0/ });
    await user.click(pill);
    expect(handlers.onUpdate).toHaveBeenCalledWith("vortex-mod-youtube");
  });

  it("displays the installed version next to the actions when installed", () => {
    renderRow({ status: "installed", installedVersion: "1.1.2" });
    expect(screen.getByText("v1.1.2")).toBeInTheDocument();
  });

  it("exposes a disable action in the kebab menu for installed plugins", async () => {
    const user = userEvent.setup();
    const handlers = renderRow({ status: "installed" });
    await user.click(
      screen.getByRole("button", { name: /(more actions|plus d['\u2019]actions)/i }),
    );
    await user.click(screen.getByRole("menuitem", { name: /(^disable$|désactiver)/i }));
    expect(handlers.onDisable).toHaveBeenCalledWith("vortex-mod-youtube");
  });

  it("exposes an uninstall action in the kebab menu for installed plugins", async () => {
    const user = userEvent.setup();
    const handlers = renderRow({ status: "installed" });
    await user.click(
      screen.getByRole("button", { name: /(more actions|plus d['\u2019]actions)/i }),
    );
    await user.click(screen.getByRole("menuitem", { name: /(uninstall|désinstaller)/i }));
    expect(handlers.onUninstall).toHaveBeenCalledWith("vortex-mod-youtube");
  });

  it("renders official badge when official is true", () => {
    renderRow({ official: true });
    expect(screen.getByText(/(official|officiel)/i)).toBeInTheDocument();
  });

  it("does not render official badge when official is false", () => {
    renderRow({ official: false });
    expect(screen.queryByText(/^(official|officiel)$/i)).not.toBeInTheDocument();
  });

  it("shows the inactive badge when the plugin is locally disabled", () => {
    renderRow({ status: "installed" }, { isLocallyDisabled: true });
    expect(screen.getByText(/(inactive|inactif)/i)).toBeInTheDocument();
  });

  it("swaps Disable for Enable in the kebab menu when locally disabled", async () => {
    const user = userEvent.setup();
    const handlers = renderRow({ status: "installed" }, { isLocallyDisabled: true });
    await user.click(
      screen.getByRole("button", { name: /(more actions|plus d['\u2019]actions)/i }),
    );
    await user.click(screen.getByRole("menuitem", { name: /(^enable$|activer)/i }));
    expect(handlers.onEnable).toHaveBeenCalledWith("vortex-mod-youtube");
    expect(
      screen.queryByRole("menuitem", { name: /(^disable$|désactiver)/i }),
    ).not.toBeInTheDocument();
  });

  it("exposes a report-broken action in the kebab menu for installed plugins", async () => {
    const user = userEvent.setup();
    const onReportBroken = vi.fn();
    renderRow({ status: "installed" }, { onReportBroken });
    await user.click(screen.getByRole("button", { name: /(more actions|plus d['’]actions)/i }));
    await user.click(screen.getByRole("menuitem", { name: /(report broken|signaler un plugin)/i }));
    expect(onReportBroken).toHaveBeenCalledWith("vortex-mod-youtube");
  });

  it("does not render the report-broken action when no handler is provided", async () => {
    const user = userEvent.setup();
    renderRow({ status: "installed" });
    await user.click(screen.getByRole("button", { name: /(more actions|plus d['’]actions)/i }));
    expect(
      screen.queryByRole("menuitem", { name: /(report broken|signaler un plugin)/i }),
    ).not.toBeInTheDocument();
  });
});
