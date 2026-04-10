import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { StateIndicator } from '../StateIndicator';
import type { DownloadState } from '@/types/download';

const ALL_STATES: DownloadState[] = [
  'Queued', 'Downloading', 'Paused', 'Waiting', 'Retry',
  'Error', 'Completed', 'Checking', 'Extracting',
];

describe('StateIndicator', () => {
  it.each(ALL_STATES)('should render state label for %s', (state) => {
    render(<StateIndicator state={state} />);
    expect(screen.getByText(state)).toBeInTheDocument();
  });

  it('should render a colored dot', () => {
    const { container } = render(<StateIndicator state="Downloading" />);
    const dot = container.querySelector('.rounded-full');
    expect(dot).toBeInTheDocument();
    expect(dot).toHaveClass('bg-green-500');
  });

  it('should animate downloading state', () => {
    const { container } = render(<StateIndicator state="Downloading" />);
    const dot = container.querySelector('.rounded-full');
    expect(dot).toHaveClass('animate-pulse');
  });

  it('should show error state in red', () => {
    const { container } = render(<StateIndicator state="Error" />);
    const dot = container.querySelector('.rounded-full');
    expect(dot).toHaveClass('bg-red-500');
  });
});
