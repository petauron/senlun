import type { ProbeComparison } from '@/utils/admin'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, nextTick, ref, watch } from 'vue'
import { createProbeComparisonLoader } from '../networkQuality'

function comparisonRow(nodeId: string, avgLatencyMs: number): ProbeComparison {
  return {
    node_id: nodeId,
    samples: 10,
    loss_percent: 0,
    avg_latency_ms: avgLatencyMs,
  }
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('network quality probe comparison polling', () => {
  it('does not re-fetch or clear rows when dashboard node updates keep the same task id and range', async () => {
    const initialRow = comparisonRow('node-1', 12.3)
    const nodes = ref([
      {
        uuid: 'node-1',
        probes: [
          { id: 'task-a', kind: 'tcp' },
        ],
      },
    ])
    const hours = ref(1)
    const task = computed(() => nodes.value.flatMap(node => node.probes).find(probe => probe.kind === 'tcp'))

    const signals: AbortSignal[] = []
    const fetchComparison = vi.fn(async (_taskId: string, _hours: number, signal: AbortSignal) => {
      signals.push(signal)
      return [initialRow]
    })
    let summary: ProbeComparison[] = []
    let loading = false
    let error = ''

    const loader = createProbeComparisonLoader({
      fetchComparison,
      isVisible: () => true,
      onState(patch) {
        if (patch.summary !== undefined)
          summary = patch.summary
        if (patch.loading !== undefined)
          loading = patch.loading
        if (patch.error !== undefined)
          error = patch.error
      },
    })

    const stop = watch(
      [
        () => task.value?.id ?? '',
        hours,
      ],
      ([nextTaskId, nextHours]) => {
        void loader.syncQuery(nextTaskId, nextHours)
      },
      { immediate: true },
    )

    await vi.advanceTimersByTimeAsync(0)
    expect(fetchComparison).toHaveBeenCalledTimes(1)
    expect(summary).toHaveLength(1)
    expect(loading).toBe(false)
    expect(error).toBe('')

    // Simulate repeated 3-second node status polls replacing node/probe objects.
    for (let tick = 0; tick < 5; tick++) {
      nodes.value = [
        {
          uuid: 'node-1',
          probes: [
            { id: 'task-a', kind: 'tcp' },
          ],
        },
      ]
      await nextTick()
      await vi.advanceTimersByTimeAsync(3000)
    }

    expect(fetchComparison).toHaveBeenCalledTimes(1)
    expect(signals[0]?.aborted).toBe(false)
    expect(summary).toEqual([initialRow])

    stop()
    loader.dispose()
  })

  it('preserves displayed rows during 30-second background refreshes and transient background failures', async () => {
    const firstRow = comparisonRow('node-1', 10)
    const secondRow = comparisonRow('node-1', 14)
    let finishSecond!: (rows: ProbeComparison[]) => void
    let failThird!: (cause: Error) => void
    const fetchComparison = vi.fn()
      .mockResolvedValueOnce([firstRow])
      .mockImplementationOnce(() => new Promise<ProbeComparison[]>(resolve => finishSecond = resolve))
      .mockImplementationOnce(() => new Promise<ProbeComparison[]>((_, reject) => failThird = reject))

    let summary: ProbeComparison[] = []
    let error = ''
    const loader = createProbeComparisonLoader({
      fetchComparison,
      isVisible: () => true,
      onState(patch) {
        if (patch.summary !== undefined)
          summary = patch.summary
        if (patch.error !== undefined)
          error = patch.error
      },
    })

    await loader.syncQuery('task-a', 1)
    expect(summary).toEqual([firstRow])

    await vi.advanceTimersByTimeAsync(30_000)
    expect(fetchComparison).toHaveBeenCalledTimes(2)
    // Existing rows stay visible while background refresh is in flight.
    expect(summary).toEqual([firstRow])

    finishSecond([secondRow])
    await vi.advanceTimersByTimeAsync(0)
    expect(summary).toEqual([secondRow])

    await vi.advanceTimersByTimeAsync(30_000)
    failThird(new Error('temporary backend outage'))
    await vi.advanceTimersByTimeAsync(0)
    expect(error).toBe('temporary backend outage')
    expect(summary).toEqual([secondRow])

    loader.dispose()
  })

  it('aborts in-flight queries and clears stale rows when switching task, switching range, or unmounting', async () => {
    const oldRow = comparisonRow('node-old', 99)
    const staleRow = comparisonRow('node-stale', 88)
    const lateRow = comparisonRow('node-late', 55)
    const signals: AbortSignal[] = []
    let resolveFirst!: (rows: ProbeComparison[]) => void
    let resolveSecond!: (rows: ProbeComparison[]) => void
    const fetchComparison = vi.fn()
      .mockImplementationOnce((_taskId: string, _hours: number, signal: AbortSignal) => {
        signals.push(signal)
        return new Promise<ProbeComparison[]>(resolve => resolveFirst = resolve)
      })
      .mockImplementationOnce((_taskId: string, _hours: number, signal: AbortSignal) => {
        signals.push(signal)
        return new Promise<ProbeComparison[]>(resolve => resolveSecond = resolve)
      })

    let summary: ProbeComparison[] = [oldRow]
    const loader = createProbeComparisonLoader({
      fetchComparison,
      isVisible: () => true,
      onState(patch) {
        if (patch.summary !== undefined)
          summary = patch.summary
      },
    })

    const first = loader.syncQuery('task-a', 1)
    expect(summary).toEqual([])
    const second = loader.syncQuery('task-b', 6)
    expect(signals[0]?.aborted).toBe(true)

    resolveFirst([staleRow])
    await first
    expect(summary).toEqual([])

    loader.dispose()
    expect(signals[1]?.aborted).toBe(true)

    resolveSecond([lateRow])
    await second
    expect(summary).toEqual([])
  })

  it('pauses scheduled refreshes while hidden and refreshes once visible again', async () => {
    let visible = true
    const row = comparisonRow('node-1', 15)
    const fetchComparison = vi.fn().mockResolvedValue([row])
    const loader = createProbeComparisonLoader({
      fetchComparison,
      isVisible: () => visible,
      onState() {},
    })

    await loader.syncQuery('task-a', 1)
    expect(fetchComparison).toHaveBeenCalledTimes(1)

    visible = false
    loader.handleVisibilityChange()
    await vi.advanceTimersByTimeAsync(60_000)
    expect(fetchComparison).toHaveBeenCalledTimes(1)

    visible = true
    loader.handleVisibilityChange()
    await vi.advanceTimersByTimeAsync(0)
    expect(fetchComparison).toHaveBeenCalledTimes(2)

    loader.dispose()
  })
})
