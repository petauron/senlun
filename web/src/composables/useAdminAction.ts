import { ref } from 'vue'
import { ApiError } from '@/utils/api'

export type AdminActionOutcome = 'idle' | 'saved' | 'failed' | 'refresh_failed' | 'uncertain'

export interface RunAdminActionOptions {
  message?: string
  reload?: boolean
  riskyWrite?: boolean
  onCommitted?: () => void
}

export function isUncertainWriteError(cause: unknown): boolean {
  if (!(cause instanceof ApiError))
    return true
  if (cause.statusCode === undefined)
    return true
  return cause.statusCode === 504 || (cause.statusCode >= 200 && cause.statusCode < 300)
}

export function useAdminAction(refresh?: () => Promise<void>) {
  const busy = ref(false)
  const error = ref('')
  const success = ref('')
  const outcome = ref<AdminActionOutcome>('idle')
  const needsResync = ref(false)
  const uncertain = ref(false)

  function clearFeedback(): void {
    error.value = ''
    success.value = ''
    outcome.value = 'idle'
    needsResync.value = false
    uncertain.value = false
  }

  async function run(
    action: () => Promise<void>,
    messageOrOptions: string | RunAdminActionOptions = '已保存',
    reloadArg = true,
  ): Promise<AdminActionOutcome> {
    if (busy.value)
      return outcome.value
    const options: RunAdminActionOptions = typeof messageOrOptions === 'string'
      ? {
          message: messageOrOptions,
          reload: reloadArg,
        }
      : messageOrOptions
    const message = options.message ?? '已保存'
    const reload = options.reload ?? true
    const riskyWrite = options.riskyWrite ?? false

    busy.value = true
    error.value = ''
    success.value = ''
    outcome.value = 'idle'
    needsResync.value = false
    uncertain.value = false

    try {
      await action()
    }
    catch (cause) {
      if (isUncertainWriteError(cause)) {
        outcome.value = 'uncertain'
        uncertain.value = true
        needsResync.value = Boolean(refresh && reload)
        error.value = riskyWrite
          ? '请求超时或连接中断，无法确认操作是否已生效。请先重新同步核对当前状态，勿直接重复提交。'
          : '请求超时或连接中断，无法确认写入是否已生效。请先重新同步核对服务端状态后再决定是否重新提交。'
      }
      else {
        outcome.value = 'failed'
        error.value = cause instanceof Error
          ? cause.message
          : String(cause)
      }
      busy.value = false
      return outcome.value
    }

    try {
      options.onCommitted?.()
      success.value = message
      if (refresh && reload) {
        try {
          await refresh()
          outcome.value = 'saved'
        }
        catch {
          outcome.value = 'refresh_failed'
          needsResync.value = true
          error.value = '操作已提交成功，但刷新数据失败。请点击“重新同步”获取最新状态，无需重复提交。'
        }
      }
      else {
        outcome.value = 'saved'
      }
    }
    finally {
      busy.value = false
    }
    return outcome.value
  }

  async function resync(message = '已重新同步最新状态'): Promise<boolean> {
    if (busy.value || !refresh)
      return false
    busy.value = true
    error.value = ''
    try {
      await refresh()
      needsResync.value = false
      uncertain.value = false
      outcome.value = 'idle'
      success.value = message
      return true
    }
    catch (cause) {
      error.value = cause instanceof Error
        ? cause.message
        : String(cause)
      return false
    }
    finally {
      busy.value = false
    }
  }

  return {
    busy,
    error,
    success,
    outcome,
    needsResync,
    uncertain,
    clearFeedback,
    run,
    resync,
  }
}
