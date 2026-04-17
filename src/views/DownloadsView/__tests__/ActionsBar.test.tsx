import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useUiStore } from '@/stores/uiStore';
import { ActionsBar } from '../ActionsBar';
import { downloadQueries } from '@/api/queries';

const { invokeMock, toastMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  toastMock: { success: vi.fn(), error: vi.fn() },
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock('@/lib/toast', () => ({ toast: toastMock }));

function makeClient(counts?: Record<string, number>) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  if (counts) {
    queryClient.setQueryData(downloadQueries.countByState(), counts);
  }
  return queryClient;
}

function renderBar(counts?: Record<string, number>) {
  const queryClient = makeClient(counts);
  return render(
    <QueryClientProvider client={queryClient}>
      <ActionsBar />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState({ selectedDownloadIds: [], selectedDownloadId: null });
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  toastMock.success.mockReset();
  toastMock.error.mockReset();
  window.localStorage.setItem('i18nextLng', 'en');
});

describe('ActionsBar', () => {
  it('should show Pause All and Resume All when no selection', () => {
    renderBar();
    expect(screen.getByText('Pause All')).toBeInTheDocument();
    expect(screen.getByText('Resume All')).toBeInTheDocument();
  });

  it('should show selection count and actions when items selected', () => {
    useUiStore.setState({ selectedDownloadIds: ['1', '2', '3'] });
    renderBar();
    expect(screen.getByText('3 selected')).toBeInTheDocument();
    expect(screen.getByText('Cancel Selected')).toBeInTheDocument();
    expect(screen.getByText('Clear')).toBeInTheDocument();
  });

  it('should clear selection when Clear is clicked', async () => {
    const user = userEvent.setup();
    useUiStore.setState({ selectedDownloadIds: ['1', '2'] });
    renderBar();
    await user.click(screen.getByText('Clear'));
    expect(useUiStore.getState().selectedDownloadIds).toEqual([]);
  });

  it('should use the singular French label when one item is selected', () => {
    window.localStorage.setItem('i18nextLng', 'fr');
    useUiStore.setState({ selectedDownloadIds: ['1'] });
    renderBar();
    expect(screen.getByText('1 sélectionné')).toBeInTheDocument();
  });
});

describe('ActionsBar — clear completed/failed', () => {
  it('disables "Clear completed" when Completed count is 0', () => {
    renderBar({ Completed: 0, Error: 3 });
    expect(
      screen.getByRole('button', { name: /clear completed/i }),
    ).toBeDisabled();
  });

  it('disables "Clear failed" when Error count is 0', () => {
    renderBar({ Completed: 1, Error: 0 });
    expect(
      screen.getByRole('button', { name: /clear failed/i }),
    ).toBeDisabled();
  });

  it('invokes download_clear_completed with deleteFiles:false and shows success toast', async () => {
    invokeMock.mockResolvedValueOnce(3);
    const user = userEvent.setup();
    renderBar({ Completed: 3, Error: 0 });

    await user.click(screen.getByRole('button', { name: /clear completed/i }));
    await user.click(await screen.findByRole('button', { name: /^clear$/i }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('download_clear_completed', {
        deleteFiles: false,
      });
    });
    await waitFor(() => {
      expect(toastMock.success).toHaveBeenCalledWith(
        expect.stringContaining('3'),
      );
    });
  });

  it('invokes download_clear_failed with deleteFiles:true when checkbox checked', async () => {
    invokeMock.mockResolvedValueOnce(2);
    const user = userEvent.setup();
    renderBar({ Completed: 0, Error: 2 });

    await user.click(screen.getByRole('button', { name: /clear failed/i }));
    await user.click(
      await screen.findByRole('checkbox', { name: /also delete files from disk/i }),
    );
    await user.click(
      screen.getByRole('button', { name: /clear and delete files/i }),
    );

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('download_clear_failed', {
        deleteFiles: true,
      });
    });
  });

  it('shows error toast when the mutation rejects', async () => {
    invokeMock.mockRejectedValueOnce(new Error('boom'));
    const user = userEvent.setup();
    renderBar({ Completed: 1, Error: 0 });

    await user.click(screen.getByRole('button', { name: /clear completed/i }));
    await user.click(await screen.findByRole('button', { name: /^clear$/i }));

    await waitFor(() => {
      expect(toastMock.error).toHaveBeenCalledWith(
        expect.stringContaining('boom'),
      );
    });
  });
});
