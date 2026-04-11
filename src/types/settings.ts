export type ProxyType = 'none' | 'http' | 'socks5';
export type ThemeMode = 'light' | 'dark' | 'auto';
export type SettingTab = 'general' | 'downloads' | 'network' | 'remote' | 'browser' | 'appearance';

export interface AppConfig {
  // General
  downloadDir: string | null;
  startMinimized: boolean;
  notificationsEnabled: boolean;
  autoExtract: boolean;
  clipboardMonitoring: boolean;
  soundEnabled: boolean;
  confirmDelete: boolean;
  subfolderPerPackage: boolean;

  // Downloads
  maxConcurrentDownloads: number;
  maxSegmentsPerDownload: number;
  speedLimitBytesPerSec: number | null;
  maxRetries: number;
  retryDelaySeconds: number;
  verifyChecksums: boolean;
  preAllocateSpace: boolean;

  // Network
  proxyType: ProxyType;
  proxyUrl: string | null;
  userAgent: string;
  dnsOverHttps: boolean;
  connectionTimeoutSeconds: number;

  // Remote Access
  webInterfaceEnabled: boolean;
  webInterfacePort: number;
  restApiEnabled: boolean;
  apiKey: string;
  websocketEnabled: boolean;

  // Browser Integration
  minFileSizeMb: number;
  excludedDomains: string[];
  excludedExtensions: string[];

  // Appearance
  theme: ThemeMode;
  accentColor: string;
  compactMode: boolean;
  locale: string;
}

export type AppConfigPatch = Partial<AppConfig>;

export const ACCENT_PRESETS = [
  { name: 'Indigo', value: '#4F46E5' },
  { name: 'Blue', value: '#0EA5E9' },
  { name: 'Purple', value: '#A855F7' },
  { name: 'Pink', value: '#EC4899' },
  { name: 'Red', value: '#EF4444' },
  { name: 'Green', value: '#10B981' },
] as const;
