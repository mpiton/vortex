import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { HistoryHeader, type HistoryHeaderProps } from '../HistoryHeader';
import type { HistoryFilterType } from '../filterEntries';

interface RenderedHeader {
  onSearchChange: ReturnType<typeof vi.fn<(next: string) => void>>;
  onFilterChange: ReturnType<typeof vi.fn<(next: HistoryFilterType) => void>>;
  onExport: ReturnType<typeof vi.fn<(format: 'csv' | 'json') => void>>;
}

function renderHeader(overrides: Partial<HistoryHeaderProps> = {}): RenderedHeader {
  const onSearchChange = vi.fn<(next: string) => void>();
  const onFilterChange = vi.fn<(next: HistoryFilterType) => void>();
  const onExport = vi.fn<(format: 'csv' | 'json') => void>();
  const props: HistoryHeaderProps = {
    search: '',
    onSearchChange,
    filter: 'all',
    onFilterChange,
    counts: { all: 3, completed: 3, failed: 0, cancelled: 0 },
    onExport,
    exportDisabled: false,
    ...overrides,
  };
  render(<HistoryHeader {...props} />);
  return { onSearchChange, onFilterChange, onExport };
}

describe('HistoryHeader', () => {
  it('should render all four filter tabs with their counts', () => {
    renderHeader();
    expect(screen.getByRole('tab', { name: /All/i })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: /Completed/i })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: /Failed/i })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: /Cancelled/i })).toBeInTheDocument();
  });

  it('should notify search change on input', async () => {
    const { onSearchChange } = renderHeader();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText('Search history'), 'hello');
    expect(onSearchChange).toHaveBeenCalledTimes(5);
    expect(onSearchChange).toHaveBeenLastCalledWith('o');
  });

  it('should notify filter change on tab click', async () => {
    const { onFilterChange } = renderHeader();
    const user = userEvent.setup();
    await user.click(screen.getByRole('tab', { name: /Failed/i }));
    expect(onFilterChange).toHaveBeenCalledWith('failed');
  });

  it('should mark the active filter tab as selected', () => {
    renderHeader({ filter: 'completed' });
    expect(
      screen.getByRole('tab', { name: /Completed/i, selected: true }),
    ).toBeInTheDocument();
  });

  it('should disable the export trigger when requested', () => {
    renderHeader({ exportDisabled: true });
    expect(screen.getByRole('button', { name: /Export/i })).toBeDisabled();
  });
});
