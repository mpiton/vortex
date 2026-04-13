import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { TooltipProvider } from '@/components/ui/tooltip';
import { downloadQueries } from '@/api/queries';
import {
  SHORTCUT_ACTION_EVENT,
  SHORTCUT_ACTIONS,
} from '@/lib/keyboardShortcuts';
import { DownloadsView } from '../DownloadsView';
import { useUiStore } from '@/stores/uiStore';
import type { DownloadView } from '@/types/download';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

const mockInvoke = vi.mocked(invoke);

const mockDownloads: DownloadView[] = [
  {
    id: '1',
    fileName: 'file1.zip',
    url: 'https://example.com/file1.zip',
    state: 'Downloading',
    progressPercent: 42,
    speedBytesPerSec: 1024,
    downloadedBytes: 4200,
    totalBytes: 10000,
    etaSeconds: 10,
    segmentsActive: 2,
    segmentsTotal: 4,
    moduleName: null,
    accountName: null,
    createdAt: Date.now(),
  },
  {
    id: '2',
    fileName: 'file2.zip',
    url: 'https://example.com/file2.zip',
    state: 'Paused',
    progressPercent: 50,
    speedBytesPerSec: 0,
    downloadedBytes: 5000,
    totalBytes: 10000,
    etaSeconds: null,
    segmentsActive: 0,
    segmentsTotal: 4,
    moduleName: null,
    accountName: null,
    createdAt: Date.now(),
  },
];

function renderWithProviders() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  queryClient.setQueryData(downloadQueries.lists(), mockDownloads);
  queryClient.setQueryData(downloadQueries.countByState(), {
    total: mockDownloads.length,
    Downloading: 1,
    Paused: 1,
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <div style={{ height: '600px' }}>
          <DownloadsView />
        </div>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe('DownloadsView', () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    mockInvoke.mockImplementation(async (command: string) => {
      switch (command) {
        case 'query_download_detail':
          return {
            id: '1',
            fileName: 'file1.zip',
            url: 'https://example.com/file1.zip',
            downloadPath: '/tmp/file1.zip',
            tempPath: '/tmp/file1.zip.part',
            state: 'Downloading',
            totalBytes: 10000,
            downloadedBytes: 4200,
            progressPercent: 42,
            speedBytesPerSec: 1024,
            etaSeconds: 10,
            segmentsActive: 2,
            segmentsTotal: 4,
            createdAt: Date.now(),
            updatedAt: Date.now(),
            moduleName: null,
            accountName: null,
            checksum: null,
            mimeType: null,
            referrer: null,
            userAgent: null,
            logs: [],
            segments: [],
          };
        default:
          return undefined;
      }
    });
    useUiStore.setState({
      selectedDownloadId: null,
      selectedDownloadIds: [],
      detailsPanelOpen: false,
      filterBarExpanded: false,
    });
  });

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

  it('should focus the search input when the global shortcut event is dispatched', async () => {
    renderWithProviders();

    const input = await screen.findByPlaceholderText('Search downloads...');
    window.dispatchEvent(
      new CustomEvent(SHORTCUT_ACTION_EVENT, {
        detail: SHORTCUT_ACTIONS.downloadsFocusSearch,
      }),
    );

    expect(input).toHaveFocus();
  });

  it('should select all downloads when the global shortcut event is dispatched', async () => {
    renderWithProviders();
    await screen.findByPlaceholderText('Search downloads...');

    await act(async () => {
      window.dispatchEvent(
        new CustomEvent(SHORTCUT_ACTION_EVENT, {
          detail: SHORTCUT_ACTIONS.downloadsSelectAll,
        }),
      );
    });

    await waitFor(() => {
      expect(useUiStore.getState().selectedDownloadIds).toEqual(['1', '2']);
    });
  });

  it('should pause active downloads and resume paused downloads when the space shortcut event is dispatched', async () => {
    useUiStore.setState({
      selectedDownloadId: null,
      selectedDownloadIds: ['1', '2'],
      detailsPanelOpen: false,
      filterBarExpanded: false,
    });

    renderWithProviders();
    await screen.findByPlaceholderText('Search downloads...');

    await act(async () => {
      window.dispatchEvent(
        new CustomEvent(SHORTCUT_ACTION_EVENT, {
          detail: SHORTCUT_ACTIONS.downloadsToggleSelected,
        }),
      );
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('download_pause', { id: '1' });
      expect(mockInvoke).toHaveBeenCalledWith('download_resume', { id: '2' });
    });
  });

  it('should remove selected downloads when the delete shortcut event is dispatched', async () => {
    useUiStore.setState({
      selectedDownloadId: null,
      selectedDownloadIds: ['1', '2'],
      detailsPanelOpen: false,
      filterBarExpanded: false,
    });

    renderWithProviders();
    await screen.findByPlaceholderText('Search downloads...');

    await act(async () => {
      window.dispatchEvent(
        new CustomEvent(SHORTCUT_ACTION_EVENT, {
          detail: SHORTCUT_ACTIONS.downloadsRemoveSelected,
        }),
      );
    });

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('download_remove', {
        id: '1',
        deleteFiles: false,
      });
      expect(mockInvoke).toHaveBeenCalledWith('download_remove', {
        id: '2',
        deleteFiles: false,
      });
    });
  });
});
