import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// Mock react-i18next globally — t(key) returns the English translation value
// so existing tests that assert on English text continue to work
vi.mock("react-i18next", () => {
  const en = {
    nav: {
      downloads: "Downloads",
      linkGrabber: "Link Grabber",
      packages: "Packages",
      accounts: "Accounts",
      captcha: "Captcha",
      plugins: "Plugins",
      scheduler: "Scheduler",
      history: "History",
      statistics: "Statistics",
      settings: "Settings",
    },
    settings: {
      tabs: {
        general: "General",
        downloads: "Downloads",
        network: "Network",
        remote: "Remote Access",
        browser: "Browser",
        appearance: "Appearance",
      },
      general: {
        title: "General",
        description: "Basic application settings",
        downloadDir: "Download directory",
        downloadDirPlaceholder: "Default download directory",
        browse: "Browse",
        startMinimized: "Start minimized",
        startMinimizedDesc: "Start the app minimized to the system tray",
        notifications: "Notifications",
        notificationsDesc: "Show desktop notifications for completed downloads",
        autoExtract: "Auto extract",
        autoExtractDesc: "Automatically extract archives after download",
        clipboardMonitoring: "Clipboard monitoring",
        clipboardMonitoringDesc: "Watch clipboard for downloadable links",
        soundEffects: "Sound effects",
        soundEffectsDesc: "Play sounds on download events",
        confirmDelete: "Confirm before delete",
        confirmDeleteDesc: "Ask for confirmation before deleting downloads",
        subfolderPerPackage: "Subfolder per package",
        subfolderPerPackageDesc: "Create a separate folder for each download package",
      },
      downloads: {
        title: "Downloads",
        description: "Download engine configuration",
        maxConcurrent: "Max concurrent downloads",
        maxSegments: "Max segments per download",
        maxSegmentsDesc: "Number of parallel connections per file",
        speedLimit: "Speed limit (MiB/s)",
        speedLimitDesc: "0 = unlimited",
        maxRetries: "Max retries",
        retryDelay: "Retry delay (seconds)",
        verifyChecksums: "Verify checksums",
        verifyChecksumsDesc: "Verify file integrity after download",
        preAllocate: "Pre-allocate space",
        preAllocateDesc: "Reserve disk space before downloading",
      },
      network: {
        title: "Network",
        description: "Proxy and connection settings",
        proxyType: "Proxy type",
        proxyNone: "None",
        proxyHttp: "HTTP",
        proxySocks5: "SOCKS5",
        proxyUrl: "Proxy URL",
        userAgent: "User agent",
        dnsOverHttps: "DNS over HTTPS",
        dnsOverHttpsDesc: "Use encrypted DNS queries",
        connectionTimeout: "Connection timeout (seconds)",
      },
      remote: {
        title: "Remote Access",
        description: "Web interface and API configuration",
        warning: "Enabling remote access exposes your download manager to the network.",
        webInterface: "Web interface",
        webInterfaceDesc: "Enable the browser-based control panel",
        webInterfacePort: "Web interface port",
        restApi: "REST API",
        restApiDesc: "Enable the HTTP REST API for third-party integrations",
        websocket: "WebSocket",
        websocketDesc: "Enable real-time WebSocket events",
        apiKey: "API Key",
        showApiKey: "Show API key",
        hideApiKey: "Hide API key",
        copyApiKey: "Copy API key",
        regenerateApiKey: "Regenerate API key",
      },
      browser: {
        title: "Browser Integration",
        description: "Browser extension capture settings",
        minFileSize: "Minimum file size (MB)",
        minFileSizeDesc: "Only capture files larger than this",
        excludedDomains: "Excluded domains",
        excludedDomainsDesc: "Comma-separated list of domains to ignore",
        excludedExtensions: "Excluded extensions",
        excludedExtensionsDesc: "Comma-separated list of file extensions to ignore",
      },
      appearance: {
        title: "Appearance",
        description: "Theme and display preferences",
        theme: "Theme",
        themeLight: "Light",
        themeDark: "Dark",
        themeAuto: "Auto",
        accentColor: "Accent color",
        customHexColor: "Custom hex color",
        customHexColorPlaceholder: "#RRGGBB",
        customHexColorInvalid: "Invalid hex color — use #RGB or #RRGGBB",
        colorPreview: "Color preview",
        compactMode: "Compact mode",
        compactModeDesc: "Reduce spacing and font sizes",
        language: "Language",
      },
    },
    downloads: {
      searchPlaceholder: "Search downloads...",
      searchAriaLabel: "Search downloads",
    },
    mediaGrabber: {
      title: "Media Grabber Options",
      failedToLoad: "Failed to load media metadata",
      noMetadata: "No metadata available for this link",
      retry: "Retry",
      cancel: "Cancel",
      download: "Download",
    },
    common: {
      retry: "Retry",
      failedToLoadSettings: "Failed to load settings",
    },
  };

  function lookupKey(obj: Record<string, unknown>, key: string): string {
    const parts = key.split(".");
    let current: unknown = obj;
    for (const part of parts) {
      if (current && typeof current === "object") {
        current = (current as Record<string, unknown>)[part];
      } else {
        return key;
      }
    }
    return typeof current === "string" ? current : key;
  }

  const t = (key: string) => lookupKey(en as unknown as Record<string, unknown>, key);

  return {
    useTranslation: () => ({
      t,
      i18n: {
        language: "en",
        changeLanguage: vi.fn().mockResolvedValue(undefined),
      },
      ready: true,
    }),
    initReactI18next: { type: "3rdParty", init: vi.fn() },
    Trans: ({ children }: { children: unknown }) => children,
  };
});

// jsdom does not implement matchMedia — provide a minimal stub for all tests
Object.defineProperty(window, "matchMedia", {
  configurable: true,
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(() => true),
  })),
});
