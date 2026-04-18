import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { TooltipProvider } from '@/components/ui/tooltip';
import { FileInfoSection } from '../FileInfoSection';
import type { DownloadDetailView } from '@/types/download';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

function mockDownloadDetail(overrides?: Partial<DownloadDetailView>): DownloadDetailView {
  return {
    id: 'dl-1',
    fileName: 'test-file.zip',
    url: 'https://example.com/test-file.zip',
    sourceHostname: 'example.com',
    state: 'Downloading',
    progressPercent: 50,
    speedBytesPerSec: 1048576,
    downloadedBytes: 524288,
    totalBytes: 1048576,
    etaSeconds: 30,
    segments: [],
    checksumExpected: null,
    destinationPath: '/home/user/Downloads/test-file.zip',
    moduleName: null,
    accountName: null,
    resumeSupported: true,
    retryCount: 0,
    maxRetries: 5,
    createdAt: Date.now(),
    updatedAt: Date.now(),
    ...overrides,
  };
}

function renderWithTooltip(ui: React.ReactElement) {
  return render(<TooltipProvider>{ui}</TooltipProvider>);
}

describe('FileInfoSection', () => {
  it('should display filename', () => {
    renderWithTooltip(<FileInfoSection download={mockDownloadDetail()} />);
    expect(screen.getAllByText('test-file.zip').length).toBeGreaterThan(0);
  });

  it('should display formatted file size', () => {
    renderWithTooltip(<FileInfoSection download={mockDownloadDetail({ totalBytes: 1048576 })} />);
    expect(screen.getByText('1.00 MB')).toBeInTheDocument();
  });

  it('should display destination path', () => {
    renderWithTooltip(<FileInfoSection download={mockDownloadDetail()} />);
    expect(
      screen.getAllByText('/home/user/Downloads/test-file.zip').length,
    ).toBeGreaterThan(0);
  });
});
