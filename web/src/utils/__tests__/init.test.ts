import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { destroyInitManager, initApp } from '../init'

const mocks = vi.hoisted(() => ({
  getDashboard: vi.fn(),
  getPublicSettings: vi.fn(),
  close: vi.fn(),
  initNodes: vi.fn(),
  clearNodes: vi.fn(),
  app: { publicSettings: null as null | { theme_settings: { dataUpdateInterval: number } }, connectionError: false, loading: true },
}))

vi.mock('@/stores/app', () => ({ useAppStore: () => mocks.app }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ loggedIn: true }) }))
vi.mock('@/stores/nodes', () => ({ useNodesStore: () => ({ initNodes: mocks.initNodes, clearNodes: mocks.clearNodes }) }))
vi.mock('@/utils/api', () => ({ ApiError: class ApiError extends Error {}, getSharedApi: () => ({ getPublicSettings: mocks.getPublicSettings }) }))
vi.mock('@/utils/rpc', () => ({ RpcError: class RpcError extends Error {}, getSharedRpc: () => ({ getDashboard: mocks.getDashboard, close: mocks.close }) }))

class FakeDocument extends EventTarget {
  visibilityState: 'visible' | 'hidden' = 'visible'
  setVisible(visible: boolean): void {
    this.visibilityState = visible ? 'visible' : 'hidden'
    this.dispatchEvent(new Event('visibilitychange'))
  }
}
let page: FakeDocument

beforeEach(() => {
  vi.useFakeTimers()
  page = new FakeDocument()
  vi.stubGlobal('document', page)
  mocks.getDashboard.mockReset().mockResolvedValue({ clients: {}, statuses: {} })
  mocks.getPublicSettings.mockReset().mockResolvedValue({ theme_settings: { dataUpdateInterval: 3 } })
  mocks.close.mockReset()
  mocks.initNodes.mockReset()
  mocks.clearNodes.mockReset()
  mocks.app.publicSettings = null
  mocks.app.connectionError = false
  mocks.app.loading = true
})

afterEach(() => {
  destroyInitManager()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('dashboard visibility', () => {
  it('waits for a background tab to become visible before loading', async () => {
    page.setVisible(false)
    await initApp()
    expect(mocks.getDashboard).not.toHaveBeenCalled()
    page.setVisible(true)
    await vi.advanceTimersByTimeAsync(0)
    expect(mocks.getDashboard).toHaveBeenCalledTimes(1)
  })

  it('stops background polling and refreshes immediately when visible again', async () => {
    await initApp()
    expect(mocks.getDashboard).toHaveBeenCalledTimes(1)
    page.setVisible(false)
    await vi.advanceTimersByTimeAsync(15_000)
    expect(mocks.getDashboard).toHaveBeenCalledTimes(1)
    page.setVisible(true)
    await vi.advanceTimersByTimeAsync(0)
    expect(mocks.getDashboard).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(3000)
    expect(mocks.getDashboard).toHaveBeenCalledTimes(3)
  })

  it('does not overlap an in-flight request when visibility changes', async () => {
    await initApp()
    let finish!: (value: { clients: object, statuses: object }) => void
    mocks.getDashboard.mockImplementationOnce(() => new Promise(resolve => finish = resolve))
    await vi.advanceTimersByTimeAsync(3000)
    page.setVisible(false)
    page.setVisible(true)
    expect(mocks.getDashboard).toHaveBeenCalledTimes(2)
    finish({ clients: {}, statuses: {} })
    await vi.advanceTimersByTimeAsync(0)
    expect(mocks.getDashboard).toHaveBeenCalledTimes(3)
  })
})
