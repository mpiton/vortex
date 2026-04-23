import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { toast } from 'sonner';
import type { HistoryView as HistoryEntry } from '@/types/download';
import { HistoryView } from '../HistoryView';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: vi.fn(),
}));

const mockInvoke = vi.mocked(invoke);
const mockSave = vi.mocked(save);
const mockToastSuccess = vi.mocked(toast.success);

function sampleEntries(): HistoryEntry[] {
  return [
    {
      entryId: '1',
      downloadId: '10',
      fileName: 'alpha.zip',
      url: 'https://a.example.com/alpha.zip',
      totalBytes: 1024,
      completedAt: Math.floor(new Date(2026, 3, 20, 12, 0, 0).getTime() / 1000),
      durationSeconds: 10,
      avgSpeed: 102,
      destinationPath: '/tmp/alpha.zip',
    },
    {
      entryId: '2',
      downloadId: '11',
      fileName: 'beta.mkv',
      url: 'https://b.example.com/beta.mkv',
      totalBytes: 5000,
      completedAt: Math.floor(new Date(2026, 3, 19, 15, 30, 0).getTime() / 1000),
      durationSeconds: 30,
      avgSpeed: 166,
      destinationPath: '/tmp/beta.mkv',
    },
  ];
}

function renderView() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: 0 } },
  });
  render(
    <QueryClientProvider client={client}>
      <HistoryView />
    </QueryClientProvider>,
  );
  return { client };
}

beforeEach(() => {
  mockInvoke.mockReset();
  mockSave.mockReset();
  mockToastSuccess.mockClear();
});

describe('HistoryView integration', () => {
  it('should replace the placeholder and render grouped entries from history_list', async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === 'history_list') return sampleEntries();
      return null;
    });

    renderView();

    await waitFor(() => {
      expect(screen.getByText('alpha.zip')).toBeInTheDocument();
      expect(screen.getByText('beta.mkv')).toBeInTheDocument();
    });
    expect(screen.queryByText(/coming soon/i)).not.toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith('history_list', undefined);
  });

  it('should render the empty state when history is empty', async () => {
    mockInvoke.mockResolvedValue([]);
    renderView();
    await waitFor(() =>
      expect(screen.getByTestId('history-empty')).toBeInTheDocument(),
    );
  });

  it('should filter to a single status tab and show 0 entries on cancelled', async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === 'history_list') return sampleEntries();
      return null;
    });
    renderView();
    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByText('alpha.zip')).toBeInTheDocument());
    await user.click(screen.getByRole('tab', { name: /Cancelled/i }));
    expect(screen.queryByText('alpha.zip')).not.toBeInTheDocument();
    expect(screen.getByTestId('history-filter-empty')).toBeInTheDocument();
  });

  it('should debounce search input and invoke history_search', async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === 'history_list') return sampleEntries();
      if (command === 'history_search') return [sampleEntries()[1]];
      return null;
    });

    renderView();
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith('history_list', undefined),
    );

    const user = userEvent.setup();
    await user.type(screen.getByLabelText('Search history'), 'beta');

    await waitFor(
      () =>
        expect(
          mockInvoke.mock.calls.some(
            ([cmd, args]) =>
              cmd === 'history_search' &&
              (args as { q: string } | undefined)?.q === 'beta',
          ),
        ).toBe(true),
      { timeout: 2000 },
    );
  }, 10_000);

  it('should open save dialog and invoke history_export on CSV export', async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === 'history_list') return sampleEntries();
      if (command === 'history_export') return 2;
      return null;
    });
    mockSave.mockResolvedValue('/tmp/history.csv');

    renderView();
    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByText('alpha.zip')).toBeInTheDocument());

    await user.click(screen.getByRole('button', { name: /Export/i }));
    await user.click(screen.getByRole('menuitem', { name: /Export as CSV/i }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith('history_export', {
        format: 'csv',
        path: '/tmp/history.csv',
      }),
    );
    await waitFor(() => expect(mockToastSuccess).toHaveBeenCalled());
  });

  it('should skip history_export when the user cancels the save dialog', async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === 'history_list') return sampleEntries();
      return null;
    });
    mockSave.mockResolvedValue(null);

    renderView();
    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByText('alpha.zip')).toBeInTheDocument());

    await user.click(screen.getByRole('button', { name: /Export/i }));
    await user.click(screen.getByRole('menuitem', { name: /Export as JSON/i }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(
      mockInvoke.mock.calls.some(([cmd]) => cmd === 'history_export'),
    ).toBe(false);
  });

  it('should re-download an entry by invoking download_start with the entry URL', async () => {
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === 'history_list') return sampleEntries();
      if (command === 'download_start') return 99;
      return null;
    });

    renderView();
    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByText('alpha.zip')).toBeInTheDocument());

    const buttons = screen.getAllByRole('button', { name: 'Re-download' });
    await user.click(buttons[0]);

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith(
        'download_start',
        expect.objectContaining({ url: expect.stringContaining('example.com') }),
      ),
    );
    await waitFor(() => expect(mockToastSuccess).toHaveBeenCalled());
  });
});
