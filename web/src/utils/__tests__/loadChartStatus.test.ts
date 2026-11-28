import type { HistoryCoverage } from '@/utils/rpc'
import { describe, expect, it } from 'vitest'
import {
  formatLoadCoverageRange,
  getLoadChartStatusDisplay,
  resolveCoverageBounds,
} from '../loadChartStatus'

function makeCoverage(overrides: Partial<HistoryCoverage> = {}): HistoryCoverage {
  const start = new Date(2026, 4, 10, 8, 0, 0).getTime()
  const end = new Date(2026, 4, 10, 12, 0, 0).getTime()
  return {
    requested_start_unix_ms: start,
    requested_end_unix_ms: end,
    actual_start_unix_ms: start,
    actual_end_unix_ms: end,
    source_points: 120,
    returned_points: 120,
    bucket_ms: 0,
    downsampled: false,
    ...overrides,
  }
}

describe('loadChartStatus helpers', () => {
  it('formats same-day, sub-minute, cross-day, and cross-year coverage ranges', () => {
    expect(
      formatLoadCoverageRange(
        new Date(2026, 4, 10, 8, 0, 0).getTime(),
        new Date(2026, 4, 10, 12, 0, 0).getTime(),
      ),
    ).toBe('05/10 08:00 – 12:00')

    expect(
      formatLoadCoverageRange(
        new Date(2026, 4, 10, 12, 0, 5).getTime(),
        new Date(2026, 4, 10, 12, 0, 35).getTime(),
      ),
    ).toBe('05/10 12:00:05 – 12:00:35')

    expect(
      formatLoadCoverageRange(
        new Date(2026, 4, 9, 12, 0, 0).getTime(),
        new Date(2026, 4, 10, 12, 0, 0).getTime(),
      ),
    ).toBe('05/09 12:00 – 05/10 12:00')

    expect(
      formatLoadCoverageRange(
        new Date(2025, 11, 31, 20, 0, 0).getTime(),
        new Date(2026, 0, 1, 0, 0, 0).getTime(),
      ),
    ).toBe('2025/12/31 20:00 – 2026/01/01 00:00')
  })

  it('keeps actual coverage distinct from requested window when actual bounds are missing', () => {
    const requestedStart = new Date(2026, 4, 10, 8, 0, 0).getTime()
    const requestedEnd = new Date(2026, 4, 10, 12, 0, 0).getTime()
    const actualStart = new Date(2026, 4, 10, 10, 15, 0).getTime()
    const actualEnd = new Date(2026, 4, 10, 11, 55, 0).getTime()

    expect(
      resolveCoverageBounds(
        makeCoverage({
          requested_start_unix_ms: requestedStart,
          requested_end_unix_ms: requestedEnd,
          actual_start_unix_ms: actualStart,
          actual_end_unix_ms: actualEnd,
        }),
        true,
      ),
    ).toEqual({
      kind: 'actual',
      start: actualStart,
      end: actualEnd,
    })

    const requestedOnlyCoverage = makeCoverage({
      requested_start_unix_ms: requestedStart,
      requested_end_unix_ms: requestedEnd,
      actual_start_unix_ms: null,
      actual_end_unix_ms: null,
    })
    expect(resolveCoverageBounds(requestedOnlyCoverage, true)).toEqual({
      kind: 'requested',
      start: requestedStart,
      end: requestedEnd,
    })

    const requestedOnlyDisplay = getLoadChartStatusDisplay({
      isRealtime: false,
      loading: false,
      error: null,
      hasRecords: true,
      coverage: requestedOnlyCoverage,
      coverageMatchesSelection: true,
    })
    expect(requestedOnlyDisplay.coverageKind).toBe('requested')
    expect(requestedOnlyDisplay.hasValidCoverage).toBe(false)
    expect(requestedOnlyDisplay.statusText).toBe('请求范围 05/10 08:00 – 12:00')

    const unknownCoverageDisplay = getLoadChartStatusDisplay({
      isRealtime: false,
      loading: false,
      error: null,
      hasRecords: true,
      coverage: null,
      coverageMatchesSelection: true,
    })
    expect(unknownCoverageDisplay.coverageKind).toBeNull()
    expect(unknownCoverageDisplay.statusText).toBe('实际覆盖时段未知')

    expect(resolveCoverageBounds(requestedOnlyCoverage, false)).toBeNull()
  })

  it('shows realtime polling vs historical snapshot for identical response coverage windows', () => {
    const coverage = makeCoverage()

    const realtime = getLoadChartStatusDisplay({
      isRealtime: true,
      loading: false,
      error: null,
      hasRecords: true,
      coverage,
      coverageMatchesSelection: true,
    })
    expect(realtime).toEqual({
      modeLabel: '自动刷新中',
      isRealtime: true,
      coverageKind: 'actual',
      rangeLabel: '05/10 08:00 – 12:00',
      statusText: '覆盖时段 05/10 08:00 – 12:00',
      hasValidCoverage: true,
    })

    const snapshot = getLoadChartStatusDisplay({
      isRealtime: false,
      loading: false,
      error: null,
      hasRecords: true,
      coverage,
      coverageMatchesSelection: true,
    })
    expect(snapshot).toEqual({
      modeLabel: '历史快照',
      isRealtime: false,
      coverageKind: 'actual',
      rangeLabel: '05/10 08:00 – 12:00',
      statusText: '覆盖时段 05/10 08:00 – 12:00',
      hasValidCoverage: true,
    })
  })

  it('never displays stale timestamps during view switches, empty responses, or errors', () => {
    const staleCoverage = makeCoverage()

    const switching = getLoadChartStatusDisplay({
      isRealtime: false,
      loading: true,
      error: null,
      hasRecords: true,
      coverage: staleCoverage,
      coverageMatchesSelection: false,
    })
    expect(switching.modeLabel).toBe('历史快照')
    expect(switching.coverageKind).toBeNull()
    expect(switching.rangeLabel).toBeNull()
    expect(switching.hasValidCoverage).toBe(false)
    expect(switching.statusText).toBe('正在读取覆盖时段…')

    const empty = getLoadChartStatusDisplay({
      isRealtime: false,
      loading: false,
      error: null,
      hasRecords: false,
      coverage: makeCoverage({
        actual_start_unix_ms: null,
        actual_end_unix_ms: null,
      }),
      coverageMatchesSelection: true,
    })
    expect(empty.coverageKind).toBeNull()
    expect(empty.rangeLabel).toBeNull()
    expect(empty.hasValidCoverage).toBe(false)
    expect(empty.statusText).toBe('当前范围暂无采样数据')

    const failed = getLoadChartStatusDisplay({
      isRealtime: true,
      loading: false,
      error: 'RPC request failed',
      hasRecords: true,
      coverage: staleCoverage,
      coverageMatchesSelection: true,
    })
    expect(failed.modeLabel).toBe('自动刷新中')
    expect(failed.coverageKind).toBeNull()
    expect(failed.rangeLabel).toBeNull()
    expect(failed.hasValidCoverage).toBe(false)
    expect(failed.statusText).toBe('暂未获取覆盖时段')
  })
})
