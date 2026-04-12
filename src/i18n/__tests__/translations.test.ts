import { describe, it, expect } from 'vitest';
import en from '../locales/en.json';
import fr from '../locales/fr.json';

type NestedRecord = { [key: string]: string | NestedRecord };

function getAllKeys(obj: NestedRecord, prefix = ''): string[] {
  return Object.entries(obj).flatMap(([key, value]) => {
    const fullKey = prefix ? `${prefix}.${key}` : key;
    if (typeof value === 'object' && value !== null) {
      return getAllKeys(value as NestedRecord, fullKey);
    }
    return [fullKey];
  });
}

describe('i18n translation files', () => {
  it('should have all English keys present in French', () => {
    const enKeys = getAllKeys(en as NestedRecord);
    const frKeys = new Set(getAllKeys(fr as NestedRecord));

    const missing = enKeys.filter((k) => !frKeys.has(k));
    expect(missing).toEqual([]);
  });

  it('should have all French keys present in English', () => {
    const frKeys = getAllKeys(fr as NestedRecord);
    const enKeys = new Set(getAllKeys(en as NestedRecord));

    const missing = frKeys.filter((k) => !enKeys.has(k));
    expect(missing).toEqual([]);
  });

  it('should have non-empty values for all English keys', () => {
    const enKeys = getAllKeys(en as NestedRecord);
    expect(enKeys.length).toBeGreaterThan(0);
  });

  it('should have non-empty values for all French keys', () => {
    const frKeys = getAllKeys(fr as NestedRecord);
    expect(frKeys.length).toBeGreaterThan(0);
  });

  it('should have nav keys for all 10 routes', () => {
    const navEn = en.nav as Record<string, string>;
    expect(Object.keys(navEn)).toHaveLength(10);
  });

  it('should have settings tabs for all 6 sections', () => {
    const tabsEn = (en.settings as Record<string, unknown>).tabs as Record<string, string>;
    expect(Object.keys(tabsEn)).toHaveLength(6);
  });

  it('should have appearance keys for theme, accent, compact and language', () => {
    const appearance = (en.settings as Record<string, unknown>).appearance as Record<string, string>;
    expect(appearance.theme).toBeDefined();
    expect(appearance.accentColor).toBeDefined();
    expect(appearance.compactMode).toBeDefined();
    expect(appearance.language).toBeDefined();
  });

  it('should have different translations for en and fr nav keys', () => {
    expect(en.nav.downloads).not.toBe(fr.nav.downloads);
    expect(en.nav.settings).not.toBe(fr.nav.settings);
  });
});
