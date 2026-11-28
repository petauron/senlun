import { describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/utils/api'
import { isUncertainWriteError, useAdminAction } from '../useAdminAction'

describe('useAdminAction submission outcome classification', () => {
  it('classifies timeouts, disconnects, 504, and unreadable 2xx responses as uncertain', () => {
    expect(isUncertainWriteError(new ApiError('API request timed out'))).toBe(true)
    expect(isUncertainWriteError(new ApiError('Failed to fetch'))).toBe(true)
    expect(isUncertainWriteError(new ApiError('Gateway timeout', 504))).toBe(true)
    expect(isUncertainWriteError(new ApiError('Invalid JSON', 200))).toBe(true)
    expect(isUncertainWriteError(new TypeError('network down'))).toBe(true)
    expect(isUncertainWriteError(new ApiError('Bad request', 400))).toBe(false)
    expect(isUncertainWriteError(new ApiError('Origin mismatch', 403))).toBe(false)
    expect(isUncertainWriteError(new ApiError('Conflict', 409))).toBe(false)
    expect(isUncertainWriteError(new ApiError('Service unavailable', 503))).toBe(false)
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

  it('commits baseline and exposes read-only resync without replaying write when refresh fails', async () => {
    const write = vi.fn().mockResolvedValue(undefined)
    const onCommitted = vi.fn()
    const refresh = vi.fn()
      .mockRejectedValueOnce(new ApiError('Refresh failed', 503))
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
    expect(action.error.value).toContain('重新同步')

    const synced = await action.resync()
    expect(synced).toBe(true)
    expect(write).toHaveBeenCalledTimes(1)
    expect(refresh).toHaveBeenCalledTimes(2)
    expect(action.needsResync.value).toBe(false)
    expect(action.error.value).toBe('')
  })

  it('flags timeout/disconnect errors as uncertain with read-only resync and high-risk guidance', async () => {
    const refresh = vi.fn().mockResolvedValue(undefined)
    const onCommitted = vi.fn()
    const action = useAdminAction(refresh)

    const outcome = await action.run(async () => {
      throw new ApiError('API request timed out')
    }, {
      message: '操作已完成',
      riskyWrite: true,
      onCommitted,
    })

    expect(outcome).toBe('uncertain')
    expect(action.uncertain.value).toBe(true)
    expect(action.needsResync.value).toBe(true)
    expect(action.error.value).toContain('勿直接重复提交')
    expect(onCommitted).not.toHaveBeenCalled()

    await action.resync()
    expect(refresh).toHaveBeenCalledTimes(1)
    expect(action.uncertain.value).toBe(false)
    expect(action.needsResync.value).toBe(false)
  })
})
