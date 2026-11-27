import type { AuthStatus } from '../auth'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { decideTrafficDeparture, resolveOAuthTotpViewState, unmountLeaveResult } from '@/utils/authFlow'
import { useAuthStore } from '../auth'

const mocks = vi.hoisted(() => {
  class MockApiError extends Error {
    readonly statusCode?: number

    constructor(message: string, statusCode?: number) {
      super(message)
      this.name = 'ApiError'
      this.statusCode = statusCode
    }
  }

  return {
    MockApiError,
    get: vi.fn(),
    post: vi.fn(),
    setCsrfToken: vi.fn(),
    invalidateSessionRequests: vi.fn(),
    updateLoginState: vi.fn(),
    clearNodes: vi.fn(),
  }
})

vi.mock('@/utils/api', () => ({
  ApiError: mocks.MockApiError,
  getSharedApi: () => ({ get: mocks.get, post: mocks.post }),
  setCsrfToken: mocks.setCsrfToken,
}))

vi.mock('@/utils/session', () => ({
  invalidateSessionRequests: mocks.invalidateSessionRequests,
}))

vi.mock('../app', () => ({
  useAppStore: () => ({ updateLoginState: mocks.updateLoginState }),
}))

vi.mock('../nodes', () => ({
  useNodesStore: () => ({ clearNodes: mocks.clearNodes }),
}))

function statusFixture(overrides: Partial<AuthStatus> = {}): AuthStatus {
  return {
    initialized: true,
    logged_in: true,
    username: 'admin',
    csrf_token: 'csrf-initial',
    totp_enabled: false,
    oauth_enabled: true,
    oauth_totp_required: false,
    ...overrides,
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
  mocks.get.mockReset()
  mocks.post.mockReset()
  mocks.setCsrfToken.mockReset()
  mocks.invalidateSessionRequests.mockReset()
  mocks.updateLoginState.mockReset()
  mocks.clearNodes.mockReset()
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('auth store session lifecycle', () => {
  it('ignores stale refresh responses that resolve after clear()', async () => {
    const auth = useAuthStore()
    let resolveStatus!: (value: AuthStatus) => void
    mocks.get.mockReturnValueOnce(new Promise<AuthStatus>((resolve) => {
      resolveStatus = resolve
    }))

    const pendingRefresh = auth.refresh()
    auth.clear()
    mocks.setCsrfToken.mockClear()
    mocks.updateLoginState.mockClear()

    resolveStatus(statusFixture({ logged_in: true, csrf_token: 'stale-csrf' }))
    const resolved = await pendingRefresh

    expect(auth.status).toBeNull()
    expect(auth.loggedIn).toBe(false)
    expect(resolved.logged_in).toBe(false)
    expect(mocks.setCsrfToken).not.toHaveBeenCalled()
    expect(mocks.updateLoginState).not.toHaveBeenCalled()
  })

  it('ignores stale refresh responses that resolve after authenticate()', async () => {
    const auth = useAuthStore()
    let resolveStatus!: (value: AuthStatus) => void
    mocks.get.mockReturnValueOnce(new Promise<AuthStatus>((resolve) => {
      resolveStatus = resolve
    }))
    mocks.post.mockResolvedValueOnce(statusFixture({ logged_in: true, csrf_token: 'fresh-csrf' }))

    const pendingRefresh = auth.refresh()
    await auth.authenticate('login', { username: 'admin', password: 'synthetic-password' })
    mocks.setCsrfToken.mockClear()
    mocks.updateLoginState.mockClear()

    resolveStatus(statusFixture({ logged_in: false, username: null, csrf_token: 'old-anon-csrf' }))
    const resolved = await pendingRefresh

    expect(auth.loggedIn).toBe(true)
    expect(auth.status?.csrf_token).toBe('fresh-csrf')
    expect(resolved.csrf_token).toBe('fresh-csrf')
    expect(mocks.setCsrfToken).not.toHaveBeenCalled()
    expect(mocks.updateLoginState).not.toHaveBeenCalled()
  })

  it('does not let a refresh started during login cancel authenticate() or overwrite the new session', async () => {
    const auth = useAuthStore()
    let resolveLogin!: (value: AuthStatus) => void
    let resolveStatus!: (value: AuthStatus) => void
    mocks.post.mockReturnValueOnce(new Promise<AuthStatus>((resolve) => {
      resolveLogin = resolve
    }))
    mocks.get.mockReturnValueOnce(new Promise<AuthStatus>((resolve) => {
      resolveStatus = resolve
    }))

    const pendingLogin = auth.authenticate('login', { username: 'admin', password: 'synthetic-password' })
    const midLoginRefresh = auth.refresh()

    resolveLogin(statusFixture({ logged_in: true, csrf_token: 'session-csrf' }))
    await pendingLogin
    expect(auth.loggedIn).toBe(true)
    expect(auth.status?.csrf_token).toBe('session-csrf')

    mocks.setCsrfToken.mockClear()
    mocks.updateLoginState.mockClear()
    resolveStatus(statusFixture({ logged_in: false, username: null, csrf_token: 'stale-anon' }))
    const resolved = await midLoginRefresh

    expect(auth.loggedIn).toBe(true)
    expect(auth.status?.csrf_token).toBe('session-csrf')
    expect(resolved.csrf_token).toBe('session-csrf')
    expect(mocks.setCsrfToken).not.toHaveBeenCalled()
    expect(mocks.updateLoginState).not.toHaveBeenCalled()
  })

  it('rejects authenticate() instead of succeeding when clear() invalidates it mid-flight', async () => {
    const auth = useAuthStore()
    let resolveLogin!: (value: AuthStatus) => void
    mocks.post.mockReturnValueOnce(new Promise<AuthStatus>((resolve) => {
      resolveLogin = resolve
    }))

    const pendingLogin = auth.authenticate('login', { username: 'admin', password: 'synthetic-password' })
    auth.clear()
    resolveLogin(statusFixture({ logged_in: true, csrf_token: 'aborted-csrf' }))

    await expect(pendingLogin).rejects.toThrow('登录状态已变更')
    expect(auth.loggedIn).toBe(false)
    expect(auth.status).toBeNull()
  })

  it('clears local auth on logout 401/403 only after auth/status confirms signed out', async () => {
    const auth = useAuthStore()
    mocks.get.mockResolvedValueOnce(statusFixture({ logged_in: true }))
    await auth.refresh()

    mocks.post.mockRejectedValueOnce(new mocks.MockApiError('Unauthorized', 401))
    mocks.get.mockResolvedValueOnce(statusFixture({ logged_in: false, username: null, csrf_token: 'anon' }))

    await auth.logout()

    expect(mocks.post).toHaveBeenCalledTimes(1)
    expect(mocks.get).toHaveBeenCalledTimes(2)
    expect(auth.status).toBeNull()
    expect(auth.loggedIn).toBe(false)
  })

  it('refreshes CSRF and retries logout once when 403 leaves the server session active', async () => {
    const auth = useAuthStore()
    mocks.get.mockResolvedValueOnce(statusFixture({ logged_in: true, csrf_token: 'stale-csrf' }))
    await auth.refresh()

    mocks.post
      .mockRejectedValueOnce(new mocks.MockApiError('Invalid CSRF token', 403))
      .mockResolvedValueOnce({ ok: true })
    mocks.get.mockResolvedValueOnce(statusFixture({ logged_in: true, csrf_token: 'rotated-csrf' }))

    await auth.logout()

    expect(mocks.setCsrfToken).toHaveBeenCalledWith('rotated-csrf')
    expect(mocks.post).toHaveBeenCalledTimes(2)
    expect(auth.loggedIn).toBe(false)
    expect(auth.status).toBeNull()
  })

  it('does not fake sign-out when logout 403 retry still fails against an active session', async () => {
    const auth = useAuthStore()
    mocks.get.mockResolvedValueOnce(statusFixture({ logged_in: true, csrf_token: 'csrf-1' }))
    await auth.refresh()

    mocks.post
      .mockRejectedValueOnce(new mocks.MockApiError('Origin mismatch', 403))
      .mockRejectedValueOnce(new mocks.MockApiError('Origin mismatch', 403))
    mocks.get.mockResolvedValueOnce(statusFixture({ logged_in: true, csrf_token: 'csrf-2' }))

    await expect(auth.logout()).rejects.toThrow('Origin mismatch')
    expect(auth.loggedIn).toBe(true)
    expect(auth.status?.csrf_token).toBe('csrf-2')
  })

  it('rejects logout when 401/403 status verification is superseded by clear() or a newer refresh', async () => {
    const auth = useAuthStore()
    mocks.get.mockResolvedValueOnce(statusFixture({ logged_in: true, csrf_token: 'csrf-1' }))
    await auth.refresh()

    let resolveVerification!: (value: AuthStatus) => void
    mocks.post.mockRejectedValueOnce(new mocks.MockApiError('Invalid CSRF token', 403))
    mocks.get.mockReturnValueOnce(new Promise<AuthStatus>((resolve) => {
      resolveVerification = resolve
    }))

    const pendingLogout = auth.logout()
    await Promise.resolve()
    auth.clear()
    resolveVerification(statusFixture({ logged_in: true, csrf_token: 'still-active-csrf' }))

    await expect(pendingLogout).rejects.toThrow('退出登录状态校验已过期')
    expect(mocks.post).toHaveBeenCalledTimes(1)

    // A newer refresh superseding the verification must also prevent reporting signed out.
    mocks.get.mockResolvedValueOnce(statusFixture({ logged_in: true, csrf_token: 'csrf-3' }))
    await auth.refresh()
    let resolveSecondVerification!: (value: AuthStatus) => void
    mocks.post.mockRejectedValueOnce(new mocks.MockApiError('Invalid CSRF token', 403))
    mocks.get
      .mockReturnValueOnce(new Promise<AuthStatus>((resolve) => {
        resolveSecondVerification = resolve
      }))
      .mockResolvedValueOnce(statusFixture({ logged_in: true, csrf_token: 'csrf-4' }))

    const secondLogout = auth.logout()
    await Promise.resolve()
    const newerRefresh = auth.refresh(true)
    await newerRefresh
    resolveSecondVerification(statusFixture({ logged_in: false, username: null, csrf_token: 'stale' }))

    await expect(secondLogout).rejects.toThrow('退出登录状态校验已过期')
    expect(auth.loggedIn).toBe(true)
    expect(auth.status?.csrf_token).toBe('csrf-4')
  })
})

describe('traffic draft guard and OAuth TOTP recovery helpers', () => {
  it('protects unsaved drafts and busy saves during an active session, but allows logout and 401 exits', () => {
    expect(decideTrafficDeparture({
      loggedIn: true,
      stayingOnTrafficSection: false,
      hasUnsavedChanges: true,
      busy: false,
    })).toBe('confirm')
    expect(decideTrafficDeparture({
      loggedIn: true,
      stayingOnTrafficSection: false,
      hasUnsavedChanges: true,
      busy: true,
    })).toBe('block')
    expect(decideTrafficDeparture({
      loggedIn: true,
      stayingOnTrafficSection: false,
      hasUnsavedChanges: false,
      busy: false,
    })).toBe('allow')
    expect(decideTrafficDeparture({
      loggedIn: false,
      stayingOnTrafficSection: false,
      hasUnsavedChanges: true,
      busy: true,
    })).toBe('allow')
    expect(unmountLeaveResult(true)).toBe(false)
    expect(unmountLeaveResult(false)).toBe(true)
  })

  it('recovers to standard login when oauth_totp=required is stale after server expiry', () => {
    expect(resolveOAuthTotpViewState('required', null, false)).toEqual({
      active: true,
      expired: false,
    })
    expect(resolveOAuthTotpViewState('required', statusFixture({
      logged_in: false,
      username: null,
      oauth_totp_required: true,
    }), false)).toEqual({
      active: true,
      expired: false,
    })
    expect(resolveOAuthTotpViewState('required', statusFixture({
      logged_in: false,
      username: null,
      oauth_totp_required: false,
    }), false)).toEqual({
      active: false,
      expired: true,
    })
    expect(resolveOAuthTotpViewState('required', statusFixture({
      logged_in: false,
      username: null,
      oauth_totp_required: true,
    }), true)).toEqual({
      active: false,
      expired: false,
    })
  })
})
