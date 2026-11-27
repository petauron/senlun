import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { ApiError, getSharedApi, setCsrfToken } from '@/utils/api'
import { invalidateSessionRequests } from '@/utils/session'
import { useAppStore } from './app'
import { useNodesStore } from './nodes'

export interface AuthStatus {
  initialized: boolean
  logged_in: boolean
  username: string | null
  csrf_token: string
  totp_enabled: boolean
  oauth_enabled: boolean
  oauth_totp_required: boolean
}

export const useAuthStore = defineStore('auth', () => {
  const status = ref<AuthStatus | null>(null)
  const error = ref('')
  const epoch = ref(0)
  const loggedIn = computed(() => status.value?.logged_in === true)
  let requestSequence = 0
  let pending: { sequence: number, promise: Promise<AuthStatus> } | null = null

  function accept(value: AuthStatus): AuthStatus {
    status.value = value
    setCsrfToken(value.csrf_token)
    useAppStore().updateLoginState(value.logged_in)
    error.value = ''
    return value
  }

  async function refresh(force = false): Promise<AuthStatus> {
    if (!force && pending)
      return pending.promise
    const sequence = ++requestSequence
    const promise = getSharedApi().get<AuthStatus>('auth/status').then((value) => {
      if (sequence !== requestSequence) {
        return status.value ?? {
          ...value,
          logged_in: false,
          username: null,
          csrf_token: '',
          oauth_totp_required: false,
        }
      }
      return accept(value)
    }).catch((cause) => {
      if (sequence === requestSequence) {
        error.value = cause instanceof Error
          ? cause.message
          : String(cause)
      }
      throw cause
    }).finally(() => {
      if (pending?.sequence === sequence)
        pending = null
    })
    pending = { sequence, promise }
    return promise
  }

  function clear(): void {
    requestSequence += 1
    pending = null
    invalidateSessionRequests()
    status.value = null
    setCsrfToken('')
    useAppStore().updateLoginState(false)
    useNodesStore().clearNodes()
    epoch.value += 1
  }

  async function authenticate(path: 'setup' | 'login' | 'oauth/complete', body: unknown): Promise<void> {
    const sequence = ++requestSequence
    pending = null
    const next = await getSharedApi().post<AuthStatus>(`auth/${path}`, body)
    if (sequence !== requestSequence)
      return
    accept(next)
    invalidateSessionRequests()
    useNodesStore().clearNodes()
    epoch.value += 1
  }

  async function logout(): Promise<void> {
    try {
      await getSharedApi().post('auth/logout')
      clear()
    }
    catch (cause) {
      if (!(cause instanceof ApiError) || (cause.statusCode !== 401 && cause.statusCode !== 403))
        throw cause
      const latest = await refresh(true)
      if (!latest.logged_in) {
        clear()
        return
      }
      await getSharedApi().post('auth/logout')
      clear()
    }
  }

  return {
    status,
    error,
    epoch,
    loggedIn,
    refresh,
    clear,
    authenticate,
    logout,
  }
})
