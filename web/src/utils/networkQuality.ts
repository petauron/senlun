import type { ProbeComparison } from '@/utils/admin'

export interface ProbeComparisonStatePatch {
  summary?: ProbeComparison[]
  loading?: boolean
  error?: string
}

export interface ProbeComparisonLoaderOptions {
  fetchComparison: (taskId: string, hours: number, signal: AbortSignal) => Promise<ProbeComparison[]>
  isVisible: () => boolean
  onState: (patch: ProbeComparisonStatePatch) => void
  refreshIntervalMs?: number
}

export interface ProbeComparisonLoader {
  syncQuery: (taskId: string, hours: number) => Promise<void>
  refresh: () => Promise<void>
  handleVisibilityChange: () => void
  dispose: () => void
}

export function createProbeComparisonLoader(options: ProbeComparisonLoaderOptions): ProbeComparisonLoader {
  const intervalMs = options.refreshIntervalMs ?? 30_000
  let activeTaskId = ''
  let activeHours = 0
  let initialized = false
  let loading = false
  let disposed = false
  let generation = 0
  let controller: AbortController | null = null
  let refreshTimer: ReturnType<typeof setTimeout> | undefined

  function clearTimer(): void {
    clearTimeout(refreshTimer)
    refreshTimer = undefined
  }

  function scheduleRefresh(): void {
    clearTimer()
    if (!disposed && activeTaskId && options.isVisible())
      refreshTimer = setTimeout(() => void run(false), intervalMs)
  }

  async function run(resetRows: boolean): Promise<void> {
    if (disposed)
      return
    clearTimer()
    controller?.abort()
    const current = ++generation
    const taskId = activeTaskId
    const hours = activeHours
    if (!taskId) {
      controller = null
      loading = false
      options.onState({ summary: [], loading: false, error: '' })
      return
    }
    const nextController = new AbortController()
    controller = nextController
    loading = true
    options.onState(resetRows
      ? { summary: [], loading: true, error: '' }
      : { loading: true, error: '' })
    try {
      const rows = await options.fetchComparison(taskId, hours, nextController.signal)
      if (current === generation && !disposed)
        options.onState({ summary: rows })
    }
    catch (cause) {
      if (current === generation && !disposed) {
        const message = cause instanceof Error
          ? cause.message
          : '读取探测汇总失败'
        options.onState(resetRows
          ? { summary: [], error: message }
          : { error: message })
      }
    }
    finally {
      if (current === generation && !disposed) {
        loading = false
        scheduleRefresh()
      }
    }
  }

  return {
    async syncQuery(taskId: string, hours: number): Promise<void> {
      if (initialized && taskId === activeTaskId && hours === activeHours)
        return
      initialized = true
      activeTaskId = taskId
      activeHours = hours
      await run(true)
    },
    async refresh(): Promise<void> {
      if (!activeTaskId)
        return
      await run(false)
    },
    handleVisibilityChange(): void {
      if (disposed)
        return
      if (!options.isVisible()) {
        clearTimer()
      }
      else if (!loading && activeTaskId) {
        void run(false)
      }
    },
    dispose(): void {
      disposed = true
      clearTimer()
      ++generation
      controller?.abort()
      controller = null
      loading = false
    },
  }
}
