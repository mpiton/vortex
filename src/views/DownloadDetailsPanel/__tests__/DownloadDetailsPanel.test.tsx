import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { TooltipProvider } from '@/components/ui/tooltip';
import { DownloadDetailsPanel } from '../DownloadDetailsPanel';
import type { DownloadDetailView } from '@/types/download';

// vi.hoisted ensures variables are available when vi.mock factories run
const { mockInvoke, uiState } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
  uiState: {
    selectedDownloadId: null as string | null,
    detailsPanelOpen: false,
  },
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (selector: (s: {
    selectedDownloadId: string | null;
    selectedDownloadIds: string[];
    detailsPanelOpen: boolean;
    filterBarExpanded: boolean;
    selectDownload: () => void;
    setSelectedDownloadIds: () => void;
    toggleDownloadSelection: () => void;
    clearSelection: () => void;
    setDetailsPanelOpen: () => void;
    toggleFilterBar: () => void;
  }) => unknown) =>
    selector({
      get selectedDownloadId() { return uiState.selectedDownloadId; },
      get detailsPanelOpen() { return uiState.detailsPanelOpen; },
      selectedDownloadIds: [],
      filterBarExpanded: false,
      selectDownload: () => undefined,
      setSelectedDownloadIds: () => undefined,
      toggleDownloadSelection: () => undefined,
      clearSelection: () => undefined,
      setDetailsPanelOpen: () => undefined,
      toggleFilterBar: () => undefined,
    }),
}));

vi.mock('@/stores/downloadStore', () => ({
  useDownloadStore: Object.assign(
    (selector: (s: { progressMap: Record<string, unknown> }) => unknown) =>
      selector({ progressMap: {} }),
    { getState: () => ({ progressMap: {} }) },
  ),
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
    segments: [
      { id: 0, startByte: 0, endByte: 524288, downloadedBytes: 262144, state: 'Downloading' },
      { id: 1, startByte: 524288, endByte: 1048576, downloadedBytes: 262144, state: 'Downloading' },
    ],
    checksumExpected: 'abc123def456',
    destinationPath: '/home/user/Downloads/test-file.zip',
    moduleName: 'core-http',
    accountName: null,
    resumeSupported: true,
    retryCount: 0,
    maxRetries: 3,
    createdAt: Date.now(),
    updatedAt: Date.now(),
    ...overrides,
  };
}

function renderWithProviders(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{ui}</TooltipProvider>
    </QueryClientProvider>,
  );
}

describe('DownloadDetailsPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue([]);
    uiState.selectedDownloadId = null;
    uiState.detailsPanelOpen = false;
  });

  it('should return null when detailsPanelOpen is false', () => {
    uiState.detailsPanelOpen = false;
    const { container } = renderWithProviders(<DownloadDetailsPanel />);
    expect(container.firstChild).toBeNull();
  });

  it('should show placeholder when no download selected', () => {
    uiState.detailsPanelOpen = true;
    uiState.selectedDownloadId = null;
    renderWithProviders(<DownloadDetailsPanel />);
    expect(screen.getByText('Select a download to view details')).toBeInTheDocument();
  });

  it('should render detail content when download selected', async () => {
    mockInvoke.mockResolvedValue(mockDownloadDetail());
    uiState.detailsPanelOpen = true;
    uiState.selectedDownloadId = 'dl-1';

    renderWithProviders(<DownloadDetailsPanel />);
    expect(await screen.findByText('Details')).toBeInTheDocument();
  });
});
