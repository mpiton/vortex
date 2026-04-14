import { describe, it, expect, vi, beforeAll, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { GeneralSection } from '../GeneralSection';
import { DownloadsSection } from '../DownloadsSection';
import { NetworkSection } from '../NetworkSection';
import { RemoteAccessSection } from '../RemoteAccessSection';
import { BrowserSection } from '../BrowserSection';
import { AppearanceSection } from '../AppearanceSection';
import type { AppConfig } from '@/types/settings';
import { ThemeProvider } from '@/theme/theme-provider';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

beforeAll(() => {
  Element.prototype.hasPointerCapture = vi.fn().mockReturnValue(false);
  Element.prototype.setPointerCapture = vi.fn();
  Element.prototype.releasePointerCapture = vi.fn();
  Element.prototype.scrollIntoView = vi.fn();
});

const mockConfig: AppConfig = {
  downloadDir: '/tmp/downloads',
  startMinimized: false,
  notificationsEnabled: true,
  autoExtract: false,
  clipboardMonitoring: true,
  soundEnabled: false,
  confirmDelete: true,
  subfolderPerPackage: false,
  maxConcurrentDownloads: 3,
  maxSegmentsPerDownload: 8,
  speedLimitBytesPerSec: null,
  maxRetries: 3,
  retryDelaySeconds: 5,
  verifyChecksums: false,
  preAllocateSpace: true,
  proxyType: 'none',
  proxyUrl: null,
  userAgent: 'Vortex/1.0',
  dnsOverHttps: false,
  connectionTimeoutSeconds: 30,
  webInterfaceEnabled: false,
  webInterfacePort: 9666,
  restApiEnabled: false,
  apiKey: 'test-api-key-abc-123',
  websocketEnabled: false,
  minFileSizeMb: 0,
  excludedDomains: [],
  excludedExtensions: [],
  theme: 'auto',
  accentColor: '#4F46E5',
  compactMode: false,
  locale: 'en',
};

function renderWithQuery(children: React.ReactNode) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      {children}
    </QueryClientProvider>,
  );
}

function renderWithTheme(children: React.ReactNode) {
  return renderWithQuery(<ThemeProvider>{children}</ThemeProvider>);
}

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  document.documentElement.classList.remove('dark');
});

describe('GeneralSection', () => {
  it('should render download directory input', () => {
    renderWithQuery(<GeneralSection config={mockConfig} />);
    expect(screen.getByDisplayValue('/tmp/downloads')).toBeInTheDocument();
  });

  it('should render all toggle settings', () => {
    renderWithQuery(<GeneralSection config={mockConfig} />);
    expect(screen.getByText('Start minimized')).toBeInTheDocument();
    expect(screen.getByText('Notifications')).toBeInTheDocument();
    expect(screen.getByText('Auto extract')).toBeInTheDocument();
    expect(screen.getByText('Clipboard monitoring')).toBeInTheDocument();
    expect(screen.getByText('Sound effects')).toBeInTheDocument();
    expect(screen.getByText('Confirm before delete')).toBeInTheDocument();
    expect(screen.getByText('Subfolder per package')).toBeInTheDocument();
  });

  it('should render Browse button for directory picker', () => {
    renderWithQuery(<GeneralSection config={mockConfig} />);
    expect(screen.getByLabelText('Browse')).toBeInTheDocument();
  });
});

describe('DownloadsSection', () => {
  it('should render number inputs with correct values', () => {
    renderWithQuery(<DownloadsSection config={mockConfig} />);
    expect(screen.getByText('Max concurrent downloads')).toBeInTheDocument();
    expect(screen.getByText('Max segments per download')).toBeInTheDocument();
    expect(screen.getByText('Speed limit (MiB/s)')).toBeInTheDocument();
  });

  it('should render toggle settings', () => {
    renderWithQuery(<DownloadsSection config={mockConfig} />);
    expect(screen.getByText('Verify checksums')).toBeInTheDocument();
    expect(screen.getByText('Pre-allocate space')).toBeInTheDocument();
  });

  it('should cap maxConcurrentDownloads input at 20 per PRD §6.10', () => {
    renderWithQuery(<DownloadsSection config={mockConfig} />);
    const label = screen.getByText('Max concurrent downloads');
    const input = label.closest('div')?.parentElement?.querySelector('input');
    expect(input).not.toBeNull();
    expect(input?.max).toBe('20');
  });
});

describe('NetworkSection', () => {
  it('should render proxy type selector', () => {
    renderWithQuery(<NetworkSection config={mockConfig} />);
    expect(screen.getByText('Proxy type')).toBeInTheDocument();
  });

  it('should not show proxy URL when proxy type is none', () => {
    renderWithQuery(<NetworkSection config={mockConfig} />);
    expect(screen.queryByText('Proxy URL')).not.toBeInTheDocument();
  });

  it('should show proxy URL when proxy type is http', () => {
    renderWithQuery(
      <NetworkSection config={{ ...mockConfig, proxyType: 'http' }} />,
    );
    expect(screen.getByPlaceholderText('http://proxy:8080')).toBeInTheDocument();
  });

  it('should render DNS over HTTPS toggle', () => {
    renderWithQuery(<NetworkSection config={mockConfig} />);
    expect(screen.getByText('DNS over HTTPS')).toBeInTheDocument();
  });
});

describe('RemoteAccessSection', () => {
  it('should render security warning', () => {
    renderWithQuery(<RemoteAccessSection config={mockConfig} />);
    expect(screen.getByText(/remote access exposes/i)).toBeInTheDocument();
  });

  it('should not show port input when web interface is disabled', () => {
    renderWithQuery(<RemoteAccessSection config={mockConfig} />);
    expect(screen.queryByText('Web interface port')).not.toBeInTheDocument();
  });

  it('should show port input when web interface is enabled', () => {
    renderWithQuery(
      <RemoteAccessSection config={{ ...mockConfig, webInterfaceEnabled: true }} />,
    );
    expect(screen.getByText('Web interface port')).toBeInTheDocument();
  });

  it('should not show API key when REST API is disabled', () => {
    renderWithQuery(<RemoteAccessSection config={mockConfig} />);
    expect(screen.queryByText('API Key')).not.toBeInTheDocument();
  });

  it('should show masked API key when REST API is enabled', () => {
    renderWithQuery(
      <RemoteAccessSection config={{ ...mockConfig, restApiEnabled: true }} />,
    );
    expect(screen.getByText('API Key')).toBeInTheDocument();
    expect(screen.getByLabelText('Show API key')).toBeInTheDocument();
  });

  it('should reveal API key when show button clicked', async () => {
    const user = userEvent.setup();
    renderWithQuery(
      <RemoteAccessSection config={{ ...mockConfig, restApiEnabled: true }} />,
    );

    await user.click(screen.getByLabelText('Show API key'));

    expect(screen.getByDisplayValue('test-api-key-abc-123')).toBeInTheDocument();
    expect(screen.getByLabelText('Hide API key')).toBeInTheDocument();
  });
});

describe('BrowserSection', () => {
  it('should render min file size input', () => {
    renderWithQuery(<BrowserSection config={mockConfig} />);
    expect(screen.getByText('Minimum file size (MB)')).toBeInTheDocument();
  });

  it('should render domain and extension textareas', () => {
    renderWithQuery(<BrowserSection config={mockConfig} />);
    expect(screen.getByText('Excluded domains')).toBeInTheDocument();
    expect(screen.getByText('Excluded extensions')).toBeInTheDocument();
  });
});

describe('AppearanceSection', () => {
  it('should render theme selector', () => {
    renderWithTheme(<AppearanceSection config={mockConfig} />);
    expect(screen.getByText('Theme')).toBeInTheDocument();
  });

  it('should apply dark theme immediately when dark is selected', async () => {
    const user = userEvent.setup();

    renderWithTheme(<AppearanceSection config={mockConfig} />);

    await user.click(screen.getByRole('combobox', { name: 'Theme' }));
    await user.click(await screen.findByRole('option', { name: 'Dark' }));

    expect(localStorage.getItem('vortex-theme')).toBe('dark');
    expect(document.documentElement).toHaveClass('dark');
  });

  it('should render 6 accent color buttons', () => {
    renderWithTheme(<AppearanceSection config={mockConfig} />);
    expect(screen.getByLabelText('Indigo')).toBeInTheDocument();
    expect(screen.getByLabelText('Blue')).toBeInTheDocument();
    expect(screen.getByLabelText('Purple')).toBeInTheDocument();
    expect(screen.getByLabelText('Pink')).toBeInTheDocument();
    expect(screen.getByLabelText('Red')).toBeInTheDocument();
    expect(screen.getByLabelText('Green')).toBeInTheDocument();
  });

  it('should render compact mode toggle', () => {
    renderWithTheme(<AppearanceSection config={mockConfig} />);
    expect(screen.getByText('Compact mode')).toBeInTheDocument();
  });

  it('should render language selector', () => {
    renderWithTheme(<AppearanceSection config={mockConfig} />);
    expect(screen.getByText('Language')).toBeInTheDocument();
  });
});
