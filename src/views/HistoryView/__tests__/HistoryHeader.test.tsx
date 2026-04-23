import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { HistoryHeader } from '../HistoryHeader';
import type { HistoryFilterType } from '../filterEntries';

function renderHeader(overrides: Partial<Parameters<typeof HistoryHeader>[0]> = {}) {
  const props = {
    search: '',
    onSearchChange: vi.fn(),
    filter: 'all' as HistoryFilterType,
    onFilterChange: vi.fn(),
    counts: { all: 3, completed: 3, failed: 0, cancelled: 0 },
    onExport: vi.fn(),
    exportDisabled: false,
    ...overrides,
  };
  render(<HistoryHeader {...props} />);
  return props;
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
    const props = renderHeader();
    const user = userEvent.setup();
    await user.type(screen.getByLabelText('Search history'), 'hello');
    expect(props.onSearchChange).toHaveBeenCalledTimes(5);
    expect(props.onSearchChange.mock.calls.at(-1)).toEqual(['o']);
  });

  it('should notify filter change on tab click', async () => {
    const props = renderHeader();
    const user = userEvent.setup();
    await user.click(screen.getByRole('tab', { name: /Failed/i }));
    expect(props.onFilterChange).toHaveBeenCalledWith('failed');
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
