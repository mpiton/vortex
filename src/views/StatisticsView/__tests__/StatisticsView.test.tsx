import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { StatisticsView } from '../StatisticsView';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

interface MockData {
  stats?: Partial<{
    totalDownloadedBytes: number;
    totalFiles: number;
    avgSpeed: number;
    peakSpeed: number;
    successRate: number;
    dailyVolumes: { date: string; bytes: number; count: number }[];
    topHosts: { hostname: string; totalBytes: number; downloadCount: number }[];
  }>;
  modules?: { moduleName: string; downloadCount: number; totalBytes: number }[];
  history?: {
    entryId: string;
    downloadId: string;
    fileName: string;
    url: string;
    totalBytes: number;
    completedAt: number;
    durationSeconds: number;
    avgSpeed: number;
    destinationPath: string;
  }[];
}

function setupMocks(data: MockData = {}) {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'stats_get') {
      return {
        totalDownloadedBytes: 0,
        totalFiles: 0,
        avgSpeed: 0,
        peakSpeed: 0,
        successRate: 1,
        dailyVolumes: [],
        topHosts: [],
        ...data.stats,
      };
    }
    if (cmd === 'stats_top_modules') return data.modules ?? [];
    if (cmd === 'history_list') return data.history ?? [];
    return null;
  });
}

function renderView() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: 0 } },
  });
  render(
    <QueryClientProvider client={client}>
      <StatisticsView />
    </QueryClientProvider>,
  );
  return client;
}

describe('StatisticsView', () => {
  it('renders the placeholder replacement with KPI cards and charts', async () => {
    setupMocks({
      stats: {
        totalDownloadedBytes: 5_242_880,
        totalFiles: 12,
        avgSpeed: 1024 * 256,
        peakSpeed: 1024 * 1024,
        successRate: 0.92,
        dailyVolumes: [
          { date: '2026-04-20', bytes: 1024 * 1024, count: 3 },
          { date: '2026-04-21', bytes: 1024 * 1024 * 4, count: 9 },
        ],
        topHosts: [
          { hostname: 'example.com', totalBytes: 1024 * 1024 * 4, downloadCount: 8 },
          { hostname: 'mirror.org', totalBytes: 1024 * 1024, downloadCount: 4 },
        ],
      },
      modules: [
        { moduleName: 'vortex-mod-youtube', downloadCount: 8, totalBytes: 1024 * 1024 * 4 },
      ],
      history: [
        {
          entryId: '1',
          downloadId: '10',
          fileName: 'movie.mkv',
          url: 'https://example.com/movie.mkv',
          totalBytes: 1024 * 1024 * 4,
          completedAt: Math.floor(Date.now() / 1000) - 3 * 86_400,
          durationSeconds: 180,
          avgSpeed: 1024 * 200,
          destinationPath: '/tmp/movie.mkv',
        },
        {
          entryId: '2',
          downloadId: '11',
          fileName: 'archive.zip',
          url: 'https://mirror.org/archive.zip',
          totalBytes: 1024 * 1024,
          completedAt: Math.floor(Date.now() / 1000) - 1 * 86_400,
          durationSeconds: 60,
          avgSpeed: 1024 * 100,
          destinationPath: '/tmp/archive.zip',
        },
      ],
    });

    renderView();

    await waitFor(() => {
      expect(screen.getByTestId('statistics-view')).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(screen.getByText('vortex-mod-youtube')).toBeInTheDocument();
    });

    expect(screen.queryByText(/coming soon/i)).not.toBeInTheDocument();
    expect(screen.getByText('Total volume')).toBeInTheDocument();
    expect(screen.getByText('Total files')).toBeInTheDocument();
    expect(screen.getByText('Success rate')).toBeInTheDocument();

    expect(screen.getByLabelText('Daily download volume bar chart')).toBeInTheDocument();
    expect(screen.getByLabelText('Top hosts donut chart')).toBeInTheDocument();
    expect(screen.getByLabelText('Type breakdown horizontal bar chart')).toBeInTheDocument();
    expect(screen.getByLabelText('Average speed line chart')).toBeInTheDocument();
  });

  it('switches period and refetches stats with the new value', async () => {
    setupMocks();
    renderView();
    await waitFor(() => expect(screen.getByTestId('statistics-view')).toBeInTheDocument());
    expect(mockInvoke).toHaveBeenCalledWith('stats_get', { period: '7d' });

    const user = userEvent.setup();
    await user.click(screen.getByRole('tab', { name: 'Last 30 days' }));

    await waitFor(() =>
      expect(
        mockInvoke.mock.calls.some(
          ([cmd, args]) =>
            cmd === 'stats_get' && (args as { period: string }).period === '30d',
        ),
      ).toBe(true),
    );
  });

  it('shows empty hint for charts without data', async () => {
    setupMocks();
    renderView();
    await waitFor(() => expect(screen.getByTestId('statistics-view')).toBeInTheDocument());
    const empties = await screen.findAllByText('Not enough data to plot');
    expect(empties.length).toBeGreaterThanOrEqual(4);
    expect(screen.getByTestId('top-modules-empty')).toBeInTheDocument();
  });

  it('renders error state when core stats query fails', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'stats_get') throw new Error('boom');
      if (cmd === 'stats_top_modules') return [];
      if (cmd === 'history_list') return [];
      return null;
    });
    renderView();
    await waitFor(() => expect(screen.getByText('boom')).toBeInTheDocument());
    expect(screen.queryByTestId('statistics-view')).not.toBeInTheDocument();
  });

  it('renders dashboard with inline error when only secondary query fails', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'stats_get') {
        return {
          totalDownloadedBytes: 1_024,
          totalFiles: 1,
          avgSpeed: 0,
          peakSpeed: 0,
          successRate: 1,
          dailyVolumes: [],
          topHosts: [],
        };
      }
      if (cmd === 'stats_top_modules') return [];
      if (cmd === 'history_list') throw new Error('history offline');
      return null;
    });
    renderView();
    await waitFor(() => expect(screen.getByTestId('statistics-view')).toBeInTheDocument());
    expect(await screen.findByRole('alert')).toHaveTextContent('history offline');
  });
});
