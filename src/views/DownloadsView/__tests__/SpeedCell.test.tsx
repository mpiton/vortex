import { describe, it, expect, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { useDownloadStore } from '@/stores/downloadStore';
import { SpeedCell } from '../SpeedCell';

beforeEach(() => {
  useDownloadStore.setState({ progressMap: {} });
});

describe('SpeedCell', () => {
  it('should show 0 B/s when no progress data', () => {
    render(<SpeedCell downloadId="1" />);
    expect(screen.getByText('0 B/s')).toBeInTheDocument();
  });

  it('should show formatted speed from store', () => {
    useDownloadStore.setState({
      progressMap: {
        '1': {
          id: '1',
          downloadedBytes: 5000,
          totalBytes: 10000,
          speedBytesPerSec: 1024 * 1024 * 5,
          lastSampleBytes: 5000,
          lastSampleTime: Date.now(),
        },
      },
    });
    render(<SpeedCell downloadId="1" />);
    expect(screen.getByText('5.00 MB/s')).toBeInTheDocument();
  });

  it('should apply green color for speeds > 10 MB/s', () => {
    useDownloadStore.setState({
      progressMap: {
        '1': {
          id: '1',
          downloadedBytes: 5000,
          totalBytes: 10000,
          speedBytesPerSec: 1024 * 1024 * 15,
          lastSampleBytes: 5000,
          lastSampleTime: Date.now(),
        },
      },
    });
    const { container } = render(<SpeedCell downloadId="1" />);
    const span = container.querySelector('span');
    expect(span).toHaveClass('text-green-600');
  });
});
