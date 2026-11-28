import type { HistoryCoverage } from '@/utils/rpc'
import dayjs from 'dayjs'

export interface LoadChartStatusInput {
  isRealtime: boolean
  loading: boolean
  error: string | null
  hasRecords: boolean
  coverage: HistoryCoverage | null
  coverageMatchesSelection: boolean
}

export interface LoadChartStatusDisplay {
  modeLabel: '自动刷新中' | '历史快照'
  isRealtime: boolean
  coverageKind: 'actual' | 'requested' | null
  rangeLabel: string | null
  statusText: string
  hasValidCoverage: boolean
}

export interface ResolvedCoverageBounds {
  kind: 'actual' | 'requested'
  start: number
  end: number
}

function isValidTimestamp(value: number | null | undefined): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0
}

export function resolveCoverageBounds(
  coverage: HistoryCoverage | null,
  hasRecords: boolean,
): ResolvedCoverageBounds | null {
  if (!coverage || !hasRecords)
    return null

  const {
    actual_start_unix_ms: actualStart,
    actual_end_unix_ms: actualEnd,
    requested_start_unix_ms: requestedStart,
    requested_end_unix_ms: requestedEnd,
  } = coverage

  if (isValidTimestamp(actualStart) && isValidTimestamp(actualEnd) && actualStart <= actualEnd) {
    return {
      kind: 'actual',
      start: actualStart,
      end: actualEnd,
    }
  }

  if (isValidTimestamp(requestedStart) && isValidTimestamp(requestedEnd) && requestedStart <= requestedEnd) {
    return {
      kind: 'requested',
      start: requestedStart,
      end: requestedEnd,
    }
  }

  return null
}

export function formatLoadCoverageRange(startMs: number, endMs: number): string {
  const start = dayjs(startMs)
  const end = dayjs(endMs)
  if (!start.isValid() || !end.isValid())
    return ''

  if (start.year() !== end.year())
    return `${start.format('YYYY/MM/DD HH:mm')} – ${end.format('YYYY/MM/DD HH:mm')}`

  if (start.month() !== end.month() || start.date() !== end.date())
    return `${start.format('MM/DD HH:mm')} – ${end.format('MM/DD HH:mm')}`

  if (endMs - startMs < 60_000)
    return `${start.format('MM/DD HH:mm:ss')} – ${end.format('HH:mm:ss')}`

  return `${start.format('MM/DD HH:mm')} – ${end.format('HH:mm')}`
}

export function getLoadChartStatusDisplay(input: LoadChartStatusInput): LoadChartStatusDisplay {
  const modeLabel = input.isRealtime
    ? '自动刷新中'
    : '历史快照'

  if (input.error) {
    return {
      modeLabel,
      isRealtime: input.isRealtime,
      coverageKind: null,
      rangeLabel: null,
      statusText: '暂未获取覆盖时段',
      hasValidCoverage: false,
    }
  }

  if (!input.coverageMatchesSelection) {
    return {
      modeLabel,
      isRealtime: input.isRealtime,
      coverageKind: null,
      rangeLabel: null,
      statusText: input.loading
        ? '正在读取覆盖时段…'
        : '当前范围暂无采样数据',
      hasValidCoverage: false,
    }
  }

  if (!input.hasRecords) {
    return {
      modeLabel,
      isRealtime: input.isRealtime,
      coverageKind: null,
      rangeLabel: null,
      statusText: input.loading
        ? '正在读取覆盖时段…'
        : '当前范围暂无采样数据',
      hasValidCoverage: false,
    }
  }

  const bounds = resolveCoverageBounds(input.coverage, input.hasRecords)
  if (!bounds) {
    return {
      modeLabel,
      isRealtime: input.isRealtime,
      coverageKind: null,
      rangeLabel: null,
      statusText: '实际覆盖时段未知',
      hasValidCoverage: false,
    }
  }

  const rangeLabel = formatLoadCoverageRange(bounds.start, bounds.end)
  if (!rangeLabel) {
    return {
      modeLabel,
      isRealtime: input.isRealtime,
      coverageKind: null,
      rangeLabel: null,
      statusText: '实际覆盖时段未知',
      hasValidCoverage: false,
    }
  }

  const prefix = bounds.kind === 'actual'
    ? '覆盖时段'
    : '请求范围'
  return {
    modeLabel,
    isRealtime: input.isRealtime,
    coverageKind: bounds.kind,
    rangeLabel,
    statusText: `${prefix} ${rangeLabel}`,
    hasValidCoverage: bounds.kind === 'actual',
  }
}
