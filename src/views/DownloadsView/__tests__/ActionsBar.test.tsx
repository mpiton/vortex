import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useUiStore } from '@/stores/uiStore';
import { ActionsBar } from '../ActionsBar';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

function renderWithQuery(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

beforeEach(() => {
  useUiStore.setState({ selectedDownloadIds: [], selectedDownloadId: null });
});

describe('ActionsBar', () => {
  it('should show Pause All and Resume All when no selection', () => {
    renderWithQuery(<ActionsBar />);
    expect(screen.getByText('Pause All')).toBeInTheDocument();
    expect(screen.getByText('Resume All')).toBeInTheDocument();
  });

  it('should show selection count and actions when items selected', () => {
    useUiStore.setState({ selectedDownloadIds: ['1', '2', '3'] });
    renderWithQuery(<ActionsBar />);
    expect(screen.getByText('3 selected')).toBeInTheDocument();
    expect(screen.getByText('Cancel Selected')).toBeInTheDocument();
    expect(screen.getByText('Clear')).toBeInTheDocument();
  });

  it('should clear selection when Clear is clicked', async () => {
    const user = userEvent.setup();
    useUiStore.setState({ selectedDownloadIds: ['1', '2'] });
    renderWithQuery(<ActionsBar />);
    await user.click(screen.getByText('Clear'));
    expect(useUiStore.getState().selectedDownloadIds).toEqual([]);
  });

  it('should use the singular French label when one item is selected', () => {
    window.localStorage.setItem('i18nextLng', 'fr');
    useUiStore.setState({ selectedDownloadIds: ['1'] });

    renderWithQuery(<ActionsBar />);

    expect(screen.getByText('1 sélectionné')).toBeInTheDocument();
  });
});
