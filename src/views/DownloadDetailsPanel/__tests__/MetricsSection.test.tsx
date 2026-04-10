import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MetricsSection } from '../MetricsSection';
import type { DownloadDetailView } from '@/types/download';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

const mockUseDownloadStore = vi.fn();
vi.mock('@/stores/downloadStore', () => ({
  useDownloadStore: (selector: Parameters<typeof mockUseDownloadStore>[0]) =>
    mockUseDownloadStore(selector),
}));

function mockDownloadDetail(overrides?: Partial<DownloadDetailView>): DownloadDetailView {
  return {
    id: 'dl-1',
    fileName: 'test-file.zip',
    url: 'https://example.com/test-file.zip',
    state: 'Downloading',
    progressPercent: 50,
    speedBytesPerSec: 524288,
    downloadedBytes: 524288,
    totalBytes: 1048576,
    etaSeconds: 30,
    segments: [
      { id: 0, startByte: 0, endByte: 524288, downloadedBytes: 262144, state: 'Downloading' },
    ],
    checksumExpected: null,
    destinationPath: '/home/user/Downloads/test-file.zip',
    moduleName: null,
    accountName: null,
    resumeSupported: true,
    retryCount: 0,
    maxRetries: 3,
    createdAt: Date.now(),
    updatedAt: Date.now(),
    ...overrides,
  };
}

describe('MetricsSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should display speed from store when progress entry exists', () => {
    mockUseDownloadStore.mockImplementation((selector) =>
      selector({
        progressMap: {
          'dl-1': {
            id: 'dl-1',
            downloadedBytes: 786432,
            totalBytes: 1048576,
            speedBytesPerSec: 2097152,
            lastSampleBytes: 786432,
            lastSampleTime: Date.now(),
          },
        },
      }),
    );

    render(<MetricsSection download={mockDownloadDetail()} />);
    // 2097152 B/s = 2 MB/s
    expect(screen.getByText('2.00 MB/s')).toBeInTheDocument();
  });

  it('should fall back to static values when no progress entry', () => {
    mockUseDownloadStore.mockImplementation((selector) =>
      selector({ progressMap: {} }),
    );

    // download.speedBytesPerSec = 524288 = 512 KB/s
    render(<MetricsSection download={mockDownloadDetail()} />);
    expect(screen.getByText('512.00 KB/s')).toBeInTheDocument();
  });
});
