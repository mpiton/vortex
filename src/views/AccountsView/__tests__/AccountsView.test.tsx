import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import type { AccountView } from "@/types/account";
import { AccountsView } from "../AccountsView";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
  open: vi.fn(),
}));

const mockInvoke = vi.mocked(invoke);
const mockSave = vi.mocked(save);
const mockOpen = vi.mocked(open);
const mockToastSuccess = vi.mocked(toast.success);
const mockToastError = vi.mocked(toast.error);

function sampleAccounts(): AccountView[] {
  return [
    {
      id: "rd-1",
      serviceName: "real-debrid",
      username: "alice",
      accountType: "debrid",
      enabled: true,
      trafficLeft: 500_000,
      trafficTotal: 1_000_000,
      validUntil: Date.now() + 86_400_000,
      lastValidated: Date.now() - 60_000,
      createdAt: Date.now() - 86_400_000,
      credentialRef: "keyring://real-debrid/alice",
    },
    {
      id: "ad-1",
      serviceName: "alldebrid",
      username: "bob",
      accountType: "premium",
      enabled: false,
      trafficLeft: null,
      trafficTotal: null,
      validUntil: null,
      lastValidated: null,
      createdAt: Date.now() - 172_800_000,
      credentialRef: "keyring://alldebrid/bob",
    },
  ];
}

function renderView() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: 0 } },
  });
  render(
    <QueryClientProvider client={client}>
      <AccountsView />
    </QueryClientProvider>,
  );
  return { client };
}

beforeEach(() => {
  window.localStorage.setItem("i18nextLng", "en");
  mockInvoke.mockReset();
  mockSave.mockReset();
  mockOpen.mockReset();
  mockToastSuccess.mockClear();
  mockToastError.mockClear();
});

describe("AccountsView", () => {
  it("renders the accounts list returned by account_list", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "account_list") return sampleAccounts();
      return null;
    });

    renderView();

    await waitFor(() => {
      expect(screen.getByText("real-debrid")).toBeInTheDocument();
      expect(screen.getByText("alldebrid")).toBeInTheDocument();
    });
    expect(screen.queryByText(/coming soon/i)).not.toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith("account_list", expect.objectContaining({}));
  });

  it("renders the empty state when no accounts exist", async () => {
    mockInvoke.mockResolvedValue([]);
    renderView();
    await waitFor(() => expect(screen.getByTestId("accounts-empty")).toBeInTheDocument());
  });

  it("filters by category when a tab is clicked", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "account_list") return sampleAccounts();
      return null;
    });

    renderView();
    await waitFor(() => screen.getByText("real-debrid"));

    const user = userEvent.setup();
    await user.click(screen.getByTestId("accounts-filter-premium"));

    expect(screen.queryByText("real-debrid")).not.toBeInTheDocument();
    expect(screen.getByText("alldebrid")).toBeInTheDocument();
  });

  it("calls account_add then refreshes the list when the add form is submitted", async () => {
    let listCallCount = 0;
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "account_list") {
        listCallCount += 1;
        return listCallCount === 1 ? [] : sampleAccounts().slice(0, 1);
      }
      if (command === "account_add") return "rd-1";
      return null;
    });

    renderView();
    await waitFor(() => screen.getByTestId("accounts-empty"));

    const user = userEvent.setup();
    await user.click(screen.getByTestId("accounts-add-trigger"));
    await user.type(screen.getByTestId("account-add-service"), "real-debrid");
    await user.type(screen.getByTestId("account-add-username"), "alice");
    await user.type(screen.getByTestId("account-add-password"), "s3cret");
    await user.click(screen.getByTestId("account-add-submit"));

    await waitFor(() => expect(mockToastSuccess).toHaveBeenCalled());
    expect(mockInvoke).toHaveBeenCalledWith(
      "account_add",
      expect.objectContaining({
        serviceName: "real-debrid",
        username: "alice",
        password: "s3cret",
        accountType: "premium",
      }),
    );
  });

  it("opens a confirm dialog and calls account_delete on confirmation", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "account_list") return sampleAccounts();
      if (command === "account_delete") return null;
      return null;
    });

    renderView();
    await waitFor(() => screen.getByText("real-debrid"));

    const user = userEvent.setup();
    const row = screen.getByTestId("account-row-rd-1");
    const menuButton = within(row).getByTestId("account-row-menu-rd-1");
    await user.click(menuButton);
    await user.click(await screen.findByRole("menuitem", { name: /delete/i }));

    const confirmButton = await screen.findByTestId("account-delete-confirm");
    await user.click(confirmButton);

    await waitFor(() => expect(mockToastSuccess).toHaveBeenCalled());
    expect(mockInvoke).toHaveBeenCalledWith("account_delete", { id: "rd-1" });
  });

  it("disables the export trigger when there are no accounts", async () => {
    mockInvoke.mockResolvedValue([]);
    renderView();
    await waitFor(() => expect(screen.getByTestId("accounts-empty")).toBeInTheDocument());

    expect(screen.getByTestId("accounts-export-trigger")).toBeDisabled();
    expect(screen.getByTestId("accounts-import-trigger")).not.toBeDisabled();
  });

  it("invokes account_export with the chosen path and passphrase", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "account_list") return sampleAccounts();
      if (command === "account_export") {
        return { path: "/tmp/bundle.vxbundle", count: 2 };
      }
      return null;
    });
    mockSave.mockResolvedValue("/tmp/bundle.vxbundle");

    renderView();
    await waitFor(() => screen.getByText("real-debrid"));

    const user = userEvent.setup();
    await user.click(screen.getByTestId("accounts-export-trigger"));
    await user.type(screen.getByTestId("account-export-passphrase"), "my-passphrase");
    await user.type(screen.getByTestId("account-export-passphrase-confirm"), "my-passphrase");
    await user.click(screen.getByTestId("account-export-submit"));

    await waitFor(() => expect(mockToastSuccess).toHaveBeenCalled());
    expect(mockInvoke).toHaveBeenCalledWith(
      "account_export",
      expect.objectContaining({
        path: "/tmp/bundle.vxbundle",
        passphrase: "my-passphrase",
      }),
    );
  });

  it("calls account_import after the file is picked and passphrase entered", async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "account_list") return sampleAccounts();
      if (command === "account_import") {
        return { path: "/tmp/in.vxbundle", imported: 2, skippedDuplicates: 0 };
      }
      return null;
    });
    mockOpen.mockResolvedValue("/tmp/in.vxbundle");

    renderView();
    await waitFor(() => screen.getByText("real-debrid"));

    const user = userEvent.setup();
    await user.click(screen.getByTestId("accounts-import-trigger"));
    await user.click(screen.getByText(/Browse/i));
    await waitFor(() =>
      expect((screen.getByTestId("account-import-path") as HTMLInputElement).value).toBe(
        "/tmp/in.vxbundle",
      ),
    );
    await user.type(screen.getByTestId("account-import-passphrase"), "my-passphrase");
    await user.click(screen.getByTestId("account-import-submit"));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith(
        "account_import",
        expect.objectContaining({
          path: "/tmp/in.vxbundle",
          passphrase: "my-passphrase",
        }),
      ),
    );
  });
});
