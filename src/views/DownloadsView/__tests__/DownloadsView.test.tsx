import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { DownloadsView } from '../DownloadsView';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue([]),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

function renderWithProviders() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <div style={{ height: '600px' }}>
        <DownloadsView />
      </div>
    </QueryClientProvider>,
  );
}

describe('DownloadsView', () => {
  it('should render search bar', () => {
    renderWithProviders();
    expect(screen.getByPlaceholderText('Search downloads...')).toBeInTheDocument();
  });

  it('should render filter tabs', () => {
    renderWithProviders();
    expect(screen.getByText('All')).toBeInTheDocument();
    expect(screen.getByText('Active')).toBeInTheDocument();
    expect(screen.getByText('Done')).toBeInTheDocument();
  });

  it('should render actions bar with Pause All / Resume All', () => {
    renderWithProviders();
    expect(screen.getByText('Pause All')).toBeInTheDocument();
    expect(screen.getByText('Resume All')).toBeInTheDocument();
  });
});
