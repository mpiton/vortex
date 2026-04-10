import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { LogsSection } from '../LogsSection';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue([]),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

function renderWithProviders(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

describe('LogsSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should display log lines', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    vi.mocked(invoke).mockResolvedValue([
      '[INFO] Download started',
      '[INFO] Connected to server',
    ]);

    renderWithProviders(<LogsSection downloadId="dl-1" />);

    expect(await screen.findByText('[INFO] Download started')).toBeInTheDocument();
    expect(screen.getByText('[INFO] Connected to server')).toBeInTheDocument();
  });

  it('should show no logs message when empty', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    vi.mocked(invoke).mockResolvedValue([]);

    renderWithProviders(<LogsSection downloadId="dl-1" />);

    expect(await screen.findByText('No logs')).toBeInTheDocument();
  });
});
