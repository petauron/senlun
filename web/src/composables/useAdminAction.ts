import { ref } from 'vue'
import { ApiError } from '@/utils/api'

export type AdminActionOutcome = 'idle' | 'saved' | 'failed' | 'refresh_failed' | 'uncertain'

export interface RunAdminActionOptions {
  message?: string
  reload?: boolean
  riskyWrite?: boolean
  isUncertainError?: (cause: unknown) => boolean
  uncertainMessage?: (detail: string) => string
  onCommitted?: () => void
}

export interface ResyncVerificationResult {
  resolved: boolean
  message?: string
  error?: string
}

export interface ResyncAdminActionOptions {
  message?: string
  verify?: () => ResyncVerificationResult
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error
    ? cause.message
    : String(cause)
}

export function isUncertainWriteError(cause: unknown): boolean {
  if (!(cause instanceof ApiError))
    return true
  if (cause.statusCode === undefined)
    return true
  if (cause.statusCode === 502 || cause.statusCode === 504)
    return true
  if (cause.statusCode >= 200 && cause.statusCode < 300)
    return true
  if (cause.statusCode >= 500 && cause.unreadable)
    return true
  return false
}

export function useAdminAction(refresh?: () => Promise<void>) {
  const busy = ref(false)
  const error = ref('')
  const resyncError = ref('')
  const success = ref('')
  const outcome = ref<AdminActionOutcome>('idle')
  const needsResync = ref(false)
  const uncertain = ref(false)
  let baseError = ''

  function clearFeedback(): void {
    error.value = ''
    resyncError.value = ''
    success.value = ''
    outcome.value = 'idle'
    needsResync.value = false
    uncertain.value = false
    baseError = ''
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
    const canResync = Boolean(refresh && reload)

    busy.value = true
    error.value = ''
    resyncError.value = ''
    success.value = ''
    outcome.value = 'idle'
    needsResync.value = false
    uncertain.value = false
    baseError = ''

    try {
      await action()
    }
    catch (cause) {
      const detail = errorMessage(cause)
      if (isUncertainWriteError(cause) || (options.isUncertainError?.(cause) ?? false)) {
        outcome.value = 'uncertain'
        uncertain.value = true
        needsResync.value = canResync
        if (options.uncertainMessage) {
          baseError = options.uncertainMessage(detail)
        }
        else if (riskyWrite && canResync) {
          baseError = `无法确认操作是否已生效（${detail}）。请先重新同步核对当前状态，勿直接重复提交。`
        }
        else if (riskyWrite) {
          baseError = `无法确认操作是否已生效（${detail}）。请先核对实际状态后再决定是否重新执行，勿直接重复提交。`
        }
        else {
          baseError = `无法确认写入是否已生效（${detail}）。请先重新同步核对服务端状态后再决定是否重新提交。`
        }
        error.value = baseError
      }
      else {
        outcome.value = 'failed'
        error.value = detail
      }
      busy.value = false
      return outcome.value
    }

    try {
      options.onCommitted?.()
      success.value = message
      if (canResync && refresh) {
        try {
          await refresh()
          outcome.value = 'saved'
        }
        catch (cause) {
          const detail = errorMessage(cause)
          outcome.value = 'refresh_failed'
          needsResync.value = true
          baseError = `操作已提交成功，但刷新数据失败（${detail}）。请点击“重新同步”获取最新状态，无需重复提交。`
          error.value = baseError
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

  async function resync(
    messageOrOptions: string | ResyncAdminActionOptions = '已重新同步最新状态',
  ): Promise<boolean> {
    if (busy.value || !refresh)
      return false
    const options: ResyncAdminActionOptions = typeof messageOrOptions === 'string'
      ? { message: messageOrOptions }
      : messageOrOptions
    const defaultMessage = options.message ?? '已重新同步最新状态'

    busy.value = true
    resyncError.value = ''
    try {
      await refresh()
      if (options.verify) {
        const verified = options.verify()
        if (!verified.resolved) {
          if (verified.error !== undefined)
            baseError = verified.error
          error.value = baseError
          success.value = verified.message ?? ''
          return false
        }
        needsResync.value = false
        uncertain.value = false
        outcome.value = 'idle'
        baseError = ''
        error.value = ''
        success.value = verified.message ?? defaultMessage
        return true
      }
      needsResync.value = false
      uncertain.value = false
      outcome.value = 'idle'
      baseError = ''
      error.value = ''
      success.value = defaultMessage
      return true
    }
    catch (cause) {
      const detail = errorMessage(cause)
      resyncError.value = detail
      error.value = baseError
        ? `${baseError}（重新同步失败：${detail}）`
        : `重新同步失败：${detail}`
      return false
    }
    finally {
      busy.value = false
    }
  }

  return {
    busy,
    error,
    resyncError,
    success,
    outcome,
    needsResync,
    uncertain,
    clearFeedback,
    run,
    resync,
  }
}
