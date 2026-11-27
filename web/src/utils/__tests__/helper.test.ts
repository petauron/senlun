import { describe, expect, it } from 'vitest'
import {
  formatBytes,
  formatBytesPerSecond,
  formatBytesPerSecondSplit,
  formatBytesPerSecondWithConfig,
  formatBytesSplit,
  formatBytesWithConfig,
  formatTrafficQuota,
} from '../helper'

describe('byte formatting utilities', () => {
  it('formats zero, negative, and non-finite inputs safely', () => {
    for (const invalid of [0, -1, -1024, Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
      expect(formatBytes(invalid)).toBe('0 B')
      expect(formatBytesWithConfig(invalid)).toBe('0 B')
      expect(formatBytesSplit(invalid)).toEqual({ value: '0', unit: 'B' })
      expect(formatBytesWithConfig(invalid, { B: -1 })).toBe('0 KB')
      expect(formatBytesSplit(invalid, { B: -1 })).toEqual({ value: '0', unit: 'KB' })
      expect(formatBytesPerSecond(invalid)).toBe('0 B/s')
      expect(formatTrafficQuota(invalid)).toBe('0B')
    }
  })

  it('keeps sub-byte values in B instead of overflowing to PB', () => {
    expect(formatBytes(0.2)).toBe('0.2 B')
    expect(formatBytesPerSecond(0.2)).toBe('0.2 B/s')
    expect(formatBytesWithConfig(0.2)).toBe('0 B')
    expect(formatBytesWithConfig(0.2, { B: 1 })).toBe('0.2 B')
    expect(formatBytesSplit(0.2, { B: 1 })).toEqual({ value: '0.2', unit: 'B' })
    expect(formatBytesPerSecondWithConfig(0.2, { B: -1, KB: 1 })).toBe('0.0 KB/s')
    expect(formatBytesPerSecondSplit(0.2, { B: -1, KB: 1 })).toEqual({ value: '0.0', unit: 'KB/s' })
  })

  it('preserves standard unit scaling and clamps oversized values to PB', () => {
    expect(formatBytes(1)).toBe('1.0 B')
    expect(formatBytes(1024)).toBe('1.0 KB')
    expect(formatBytes(1536, 2)).toBe('1.50 KB')
    expect(formatBytesWithConfig(1536, { KB: 1 })).toBe('1.5 KB')
    expect(formatBytesSplit(1024 ** 3, { GB: 1 })).toEqual({ value: '1.0', unit: 'GB' })
    expect(formatBytes(1024 ** 5)).toBe('1.0 PB')
    expect(formatBytes(1024 ** 6)).toBe('1024.0 PB')
    expect(formatBytesWithConfig(1024 ** 6, { TB: 1 })).toBe('1024.0 PB')
    expect(formatBytesSplit(1024 ** 6, { TB: 2 })).toEqual({ value: '1024.00', unit: 'PB' })
  })
})
