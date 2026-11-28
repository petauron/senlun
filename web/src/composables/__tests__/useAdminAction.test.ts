import { describe, expect, it, vi } from 'vitest'
import { createSerializedRefresh } from '@/utils/adminRefresh'
import {
  clearAdminUncertainLocks,
  enrollUncertainLock,
  nodeDangerUncertainLocks,
  setEnrollUncertainLock,
  setNodeDangerUncertainLock,
  setTrafficRemoveUncertainLock,
  trafficRemoveUncertainLocks,
} from '@/utils/adminUncertain'
import { ApiError } from '@/utils/api'
import { getSessionGeneration, invalidateSessionRequests } from '@/utils/session'
import { isUncertainWriteError, useAdminAction } from '../useAdminAction'

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

describe('useAdminAction submission outcome classification', () => {
  it('classifies timeouts, disconnects, 502/504, unreadable 5xx, and unreadable 2xx responses as uncertain', () => {
    expect(isUncertainWriteError(new ApiError('API request timed out'))).toBe(true)
    expect(isUncertainWriteError(new ApiError('Failed to fetch'))).toBe(true)
    expect(isUncertainWriteError(new ApiError('Bad Gateway', 502))).toBe(true)
    expect(isUncertainWriteError(new ApiError('Gateway timeout', 504))).toBe(true)
    expect(isUncertainWriteError(new ApiError('服务返回了无法读取的响应（HTTP 503）', 503, true))).toBe(true)
    expect(isUncertainWriteError(new ApiError('请求失败（HTTP 500）', 500, true))).toBe(true)
    expect(isUncertainWriteError(new ApiError('Invalid JSON', 200, true))).toBe(true)
    expect(isUncertainWriteError(new TypeError('network down'))).toBe(true)

    expect(isUncertainWriteError(new ApiError('Bad request', 400))).toBe(false)
    expect(isUncertainWriteError(new ApiError('Origin mismatch', 403))).toBe(false)
    expect(isUncertainWriteError(new ApiError('Conflict', 409))).toBe(false)
    expect(isUncertainWriteError(new ApiError('Structured application error', 500))).toBe(false)
    expect(isUncertainWriteError(new ApiError('Structured service unavailable', 503))).toBe(false)
  })

  it('keeps definite write failures retryable without committing or refreshing', async () => {
    const refresh = vi.fn().mockResolvedValue(undefined)
    const onCommitted = vi.fn()
    const action = useAdminAction(refresh)

    const outcome = await action.run(async () => {
      throw new ApiError('节点名称无效', 400)
    }, {
      message: '已保存',
      onCommitted,
    })

    expect(outcome).toBe('failed')
    expect(action.error.value).toBe('节点名称无效')
    expect(action.uncertain.value).toBe(false)
    expect(action.needsResync.value).toBe(false)
    expect(onCommitted).not.toHaveBeenCalled()
    expect(refresh).not.toHaveBeenCalled()
  })

  it('commits baseline, preserves original committed context on failed resync, and resyncs read-only without replaying write', async () => {
    const write = vi.fn().mockResolvedValue(undefined)
    const onCommitted = vi.fn()
    const refresh = vi.fn()
      .mockRejectedValueOnce(new ApiError('Initial refresh failed', 503))
      .mockRejectedValueOnce(new ApiError('Second refresh failed', 503))
      .mockResolvedValueOnce(undefined)
    const action = useAdminAction(refresh)

    const outcome = await action.run(write, {
      message: '已保存',
      onCommitted,
    })

    expect(outcome).toBe('refresh_failed')
    expect(onCommitted).toHaveBeenCalledTimes(1)
    expect(action.success.value).toBe('已保存')
    expect(action.needsResync.value).toBe(true)
    expect(action.uncertain.value).toBe(false)
    expect(action.error.value).toContain('操作已提交成功')
    expect(action.error.value).toContain('Initial refresh failed')

    const failedRetry = await action.resync()
    expect(failedRetry).toBe(false)
    expect(action.outcome.value).toBe('refresh_failed')
    expect(action.needsResync.value).toBe(true)
    expect(action.success.value).toBe('已保存')
    expect(action.error.value).toContain('操作已提交成功')
    expect(action.error.value).toContain('Second refresh failed')
    expect(action.resyncError.value).toBe('Second refresh failed')

    const synced = await action.resync()
    expect(synced).toBe(true)
    expect(write).toHaveBeenCalledTimes(1)
    expect(refresh).toHaveBeenCalledTimes(3)
    expect(action.needsResync.value).toBe(false)
    expect(action.error.value).toBe('')
  })

  it('preserves concrete gateway error details and respects verify results on uncertain writes', async () => {
    const refresh = vi.fn().mockResolvedValue(undefined)
    const onCommitted = vi.fn()
    const action = useAdminAction(refresh)

    const outcome = await action.run(async () => {
      throw new ApiError('Bad Gateway', 502)
    }, {
      message: '操作已完成',
      riskyWrite: true,
      onCommitted,
    })

    expect(outcome).toBe('uncertain')
    expect(action.uncertain.value).toBe(true)
    expect(action.needsResync.value).toBe(true)
    expect(action.error.value).toContain('Bad Gateway')
    expect(action.error.value).toContain('勿直接重复提交')
    expect(onCommitted).not.toHaveBeenCalled()

    const unresolved = await action.resync({
      verify: () => ({
        resolved: false,
        error: '管理状态接口无法证明凭据轮换是否已生效。',
      }),
    })
    expect(unresolved).toBe(false)
    expect(action.uncertain.value).toBe(true)
    expect(action.needsResync.value).toBe(true)
    expect(action.error.value).toContain('无法证明凭据轮换')

    const verified = await action.resync({
      verify: () => ({
        resolved: true,
        message: '已核实：目标对象已删除',
      }),
    })
    expect(verified).toBe(true)
    expect(refresh).toHaveBeenCalledTimes(2)
    expect(action.uncertain.value).toBe(false)
    expect(action.needsResync.value).toBe(false)
    expect(action.success.value).toBe('已核实：目标对象已删除')
  })

  it('avoids telling users to click resync when reload is disabled on risky writes', async () => {
    const action = useAdminAction()
    const outcome = await action.run(async () => {
      throw new ApiError('API request timed out')
    }, {
      message: '已生成注册令牌',
      reload: false,
      riskyWrite: true,
    })

    expect(outcome).toBe('uncertain')
    expect(action.needsResync.value).toBe(false)
    expect(action.error.value).toContain('API request timed out')
    expect(action.error.value).not.toContain('请先重新同步')
  })

  it('ignores late uncertain lock writes from an ended session even after re-authentication', () => {
    clearAdminUncertainLocks()
    const oldSessionGen = getSessionGeneration()

    // Session ends and a new session authenticates (each bumps generation).
    invalidateSessionRequests()
    clearAdminUncertainLocks()
    invalidateSessionRequests()

    expect(setEnrollUncertainLock(oldSessionGen, true, 'stale enroll')).toBe(false)
    expect(setNodeDangerUncertainLock(oldSessionGen, true, {
      key: 'rotate:node-1',
      action: 'rotate',
      nodeId: 'node-1',
      nodeName: 'Node 1',
      verifiable: false,
      message: 'stale rotate',
      resyncError: '',
    })).toBe(false)
    expect(setTrafficRemoveUncertainLock(oldSessionGen, true, {
      nodeId: 'node-1',
      nodeName: 'Node 1',
      message: 'stale remove',
      resyncError: '',
    })).toBe(false)

    expect(enrollUncertainLock.value).toBeNull()
    expect(nodeDangerUncertainLocks.value).toEqual({})
    expect(trafficRemoveUncertainLocks.value).toEqual({})

    const activeGen = getSessionGeneration()
    expect(setEnrollUncertainLock(activeGen, false, 'logged out')).toBe(false)
    expect(setEnrollUncertainLock(activeGen, true, 'current enroll')).toBe(true)
    expect(enrollUncertainLock.value).toBe('current enroll')

    clearAdminUncertainLocks()
    expect(enrollUncertainLock.value).toBeNull()
  })
})

describe('createSerializedRefresh controlled concurrency', () => {
  it('waits for an in-flight GET to settle, ignores stale data, and runs a fresh post-write GET', async () => {
    const firstStarted = deferred<void>()
    const first = deferred<string>()
    const secondStarted = deferred<void>()
    const second = deferred<string>()
    const applied: string[] = []
    const loadingStates: boolean[] = []
    let calls = 0

    const refresher = createSerializedRefresh<string, string>(
      async (label, isLatest) => {
        calls++
        if (label === 'first') {
          firstStarted.resolve()
          const value = await first.promise
          if (isLatest())
            applied.push(value)
          return value
        }
        secondStarted.resolve()
        const value = await second.promise
        if (isLatest())
          applied.push(value)
        return value
      },
      loading => loadingStates.push(loading),
    )

    const p1 = refresher.refresh('first')
    await firstStarted.promise
    const p2 = refresher.refresh('second')
    expect(calls).toBe(1)

    first.resolve('stale-pre-post-snapshot')
    await secondStarted.promise
    expect(applied).toEqual([])
    expect(calls).toBe(2)

    second.resolve('fresh-post-post-snapshot')
    await expect(p2).resolves.toBe('fresh-post-post-snapshot')
    await expect(p1).resolves.toBe('fresh-post-post-snapshot')
    expect(applied).toEqual(['fresh-post-post-snapshot'])
    expect(loadingStates.at(-1)).toBe(false)
  })

  it('still executes the queued fresh GET when an earlier in-flight GET fails', async () => {
    const firstStarted = deferred<void>()
    const first = deferred<string>()
    const secondStarted = deferred<void>()
    const second = deferred<string>()
    const applied: string[] = []

    const refresher = createSerializedRefresh<string, string>(async (label, isLatest) => {
      if (label === 'first') {
        firstStarted.resolve()
        const value = await first.promise
        if (isLatest())
          applied.push(value)
        return value
      }
      secondStarted.resolve()
      const value = await second.promise
      if (isLatest())
        applied.push(value)
      return value
    })

    const p1 = refresher.refresh('first')
    await firstStarted.promise
    const p2 = refresher.refresh('second')

    first.reject(new ApiError('Stale GET failed', 503))
    await expect(p1).rejects.toThrow('Stale GET failed')
    await secondStarted.promise

    second.resolve('fresh-after-error')
    await expect(p2).resolves.toBe('fresh-after-error')
    expect(applied).toEqual(['fresh-after-error'])
  })

  it('merges same-tick unstarted reads into the latest fetch and propagates final failure cleanly', async () => {
    const latestStarted = deferred<void>()
    const latest = deferred<string>()
    const fetched: string[] = []

    const refresher = createSerializedRefresh<string, string>(async (label) => {
      fetched.push(label)
      latestStarted.resolve()
      return latest.promise
    })

    const p1 = refresher.refresh('unstarted-first')
    const p2 = refresher.refresh('latest-second')
    await latestStarted.promise

    latest.reject(new ApiError('Latest GET failed', 503))
    await expect(p2).rejects.toThrow('Latest GET failed')
    await expect(p1).rejects.toThrow('Latest GET failed')
    expect(fetched).toEqual(['latest-second'])
  })

  it('never lets an intermediate queued resync verify against stale state when a 3rd queued read fails', async () => {
    const firstStarted = deferred<void>()
    const first = deferred<string>()
    const thirdStarted = deferred<void>()
    const third = deferred<string>()
    let appliedState = 'stale-initial-state'
    const fetched: string[] = []

    const refresher = createSerializedRefresh<string, string>(async (label, isLatest) => {
      fetched.push(label)
      if (label === 'first') {
        firstStarted.resolve()
        const value = await first.promise
        if (isLatest())
          appliedState = value
        return value
      }
      thirdStarted.resolve()
      const value = await third.promise
      if (isLatest())
        appliedState = value
      return value
    })

    const action = useAdminAction(async () => {
      await refresher.refresh('second-resync')
    })

    await action.run(async () => {
      throw new ApiError('Bad Gateway', 502)
    }, {
      riskyWrite: true,
    })

    const p1 = refresher.refresh('first')
    await firstStarted.promise
    const verify = vi.fn(() => ({
      resolved: true,
      message: `verified:${appliedState}`,
    }))
    const resyncPromise = action.resync({ verify })
    const p3 = refresher.refresh('third-fails')

    first.resolve('stale-first-snapshot')
    await thirdStarted.promise

    third.reject(new ApiError('Final GET failed', 503))
    await expect(p3).rejects.toThrow('Final GET failed')
    await expect(p1).rejects.toThrow('Final GET failed')
    await expect(resyncPromise).resolves.toBe(false)

    // Intermediate skipped read must NOT run verify against stale state or clear uncertain lock.
    expect(fetched).toEqual(['first', 'third-fails'])
    expect(verify).not.toHaveBeenCalled()
    expect(action.uncertain.value).toBe(true)
    expect(action.needsResync.value).toBe(true)
    expect(appliedState).toBe('stale-initial-state')
  })

  it('resolves intermediate queued resync only after a 3rd queued read actually applies fresh state', async () => {
    const firstStarted = deferred<void>()
    const first = deferred<string>()
    const thirdStarted = deferred<void>()
    const third = deferred<string>()
    let appliedState = 'stale-initial-state'

    const refresher = createSerializedRefresh<string, string>(async (label, isLatest) => {
      if (label === 'first') {
        firstStarted.resolve()
        const value = await first.promise
        if (isLatest())
          appliedState = value
        return value
      }
      thirdStarted.resolve()
      const value = await third.promise
      if (isLatest())
        appliedState = value
      return value
    })

    const action = useAdminAction(async () => {
      await refresher.refresh('second-resync')
    })

    await action.run(async () => {
      throw new ApiError('API request timed out')
    }, {
      riskyWrite: true,
    })

    const p1 = refresher.refresh('first')
    await firstStarted.promise
    let observedDuringVerify = ''
    const resyncPromise = action.resync({
      verify: () => {
        observedDuringVerify = appliedState
        return {
          resolved: true,
          message: '已核实最新状态',
        }
      },
    })
    const p3 = refresher.refresh('third-succeeds')

    first.resolve('stale-first-snapshot')
    await thirdStarted.promise
    expect(observedDuringVerify).toBe('')

    third.resolve('fresh-third-snapshot')
    await expect(p3).resolves.toBe('fresh-third-snapshot')
    await expect(p1).resolves.toBe('fresh-third-snapshot')
    await expect(resyncPromise).resolves.toBe(true)
    expect(observedDuringVerify).toBe('fresh-third-snapshot')
    expect(action.uncertain.value).toBe(false)
  })
})
