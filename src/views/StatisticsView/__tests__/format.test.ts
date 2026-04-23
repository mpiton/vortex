import { describe, expect, it } from 'vitest';
import {
  formatBytes,
  formatCount,
  formatDurationFromSeconds,
  formatPercent,
  formatSpeed,
} from '../format';

describe('formatBytes', () => {
  it.each([
    [0, '0 B'],
    [-10, '0 B'],
    [512, '512 B'],
    [2048, '2.0 KiB'],
    [1024 * 1024 * 5, '5.0 MiB'],
    [1024 * 1024 * 150, '150 MiB'],
    [1024 * 1024 * 1024 * 3, '3.0 GiB'],
  ])('formats %i as %s', (bytes, expected) => {
    expect(formatBytes(bytes)).toBe(expected);
  });
});

describe('formatSpeed', () => {
  it('appends /s', () => {
    expect(formatSpeed(1024 * 1024)).toBe('1.0 MiB/s');
  });
});

describe('formatPercent', () => {
  it('formats ratio as percent with 1 decimal', () => {
    expect(formatPercent(0.9512)).toBe('95.1%');
  });
  it('returns em dash for non-finite', () => {
    expect(formatPercent(Number.NaN)).toBe('—');
  });
});

describe('formatCount', () => {
  it('uses locale separators', () => {
    expect(formatCount(1_234)).toMatch(/1[\s ,.]?234/);
  });
});

describe('formatDurationFromSeconds', () => {
  it.each([
    [0, '0h'],
    [-100, '0h'],
    [60 * 30, '30min'],
    [3_600, '1h'],
    [3_600 * 2 + 60 * 5, '2h05'],
  ])('formats %i seconds as %s', (input, expected) => {
    expect(formatDurationFromSeconds(input)).toBe(expected);
  });
});
