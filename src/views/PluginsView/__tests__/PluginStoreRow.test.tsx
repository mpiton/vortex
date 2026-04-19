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

function renderRow(override: Partial<PluginStoreEntry> = {}, handlers = {}) {
  const defaultHandlers = {
    onInstall: vi.fn(),
    onUpdate: vi.fn(),
    onDisable: vi.fn(),
    onUninstall: vi.fn(),
    isInstalling: false,
    isUpdating: false,
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

  it("shows an enabled toggle when the plugin is installed", () => {
    renderRow({ status: "installed" });
    const toggle = screen.getByRole("switch");
    expect(toggle).toHaveAttribute("data-state", "checked");
  });

  it("calls onDisable when the toggle is turned off", async () => {
    const user = userEvent.setup();
    const handlers = renderRow({ status: "installed" });
    await user.click(screen.getByRole("switch"));
    expect(handlers.onDisable).toHaveBeenCalledWith("vortex-mod-youtube");
  });

  it("renders an update pill when an update is available", async () => {
    const user = userEvent.setup();
    const handlers = renderRow({
      status: "update_available",
      version: "1.2.0",
      installedVersion: "1.1.0",
    });
    const pill = screen.getByRole("button", { name: /1\.2\.0/ });
    await user.click(pill);
    expect(handlers.onUpdate).toHaveBeenCalledWith("vortex-mod-youtube");
  });

  it("displays the installed version next to the toggle when installed", () => {
    renderRow({ status: "installed", installedVersion: "1.1.2" });
    expect(screen.getByText("v1.1.2")).toBeInTheDocument();
  });

  it("exposes an uninstall action in the kebab menu for installed plugins", async () => {
    const user = userEvent.setup();
    const handlers = renderRow({ status: "installed" });
    await user.click(screen.getByRole("button", { name: /(more actions|plus d'actions)/i }));
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
});
