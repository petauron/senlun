import type { APIRequestContext, Cookie, Page } from '@playwright/test'
import { expect } from '@playwright/test'
import { testPassword, testUsername } from './credentials.mjs'

const baseOrigin = 'http://127.0.0.1:18080'
let cachedSessionCookies: Cookie[] | null = null

async function waitForInitialized(request: APIRequestContext): Promise<string> {
  let csrfToken = ''
  await expect.poll(async () => {
    const response = await request.get('/api/auth/status')
    if (!response.ok())
      return false
    const status = await response.json()
    if (!status.initialized)
      return false
    csrfToken = status.csrf_token
    return true
  }, { timeout: 20_000 }).toBe(true)
  return csrfToken
}

async function ensureSessionCookies(page: Page): Promise<Cookie[]> {
  if (cachedSessionCookies)
    return cachedSessionCookies
  const csrfToken = await waitForInitialized(page.request)
  const response = await page.request.post('/api/auth/login', {
    headers: { 'Origin': baseOrigin, 'X-CSRF-Token': csrfToken },
    data: { username: testUsername, password: testPassword },
  })
  expect(response.ok()).toBe(true)
  const cookies = (await page.context().cookies(baseOrigin)).filter(
    cookie => cookie.name === 'senlun_session' || cookie.name === 'senlun_csrf',
  )
  expect(cookies.some(cookie => cookie.name === 'senlun_session')).toBe(true)
  cachedSessionCookies = cookies
  return cookies
}

export async function loginViaUi(page: Page, destination = '/'): Promise<void> {
  await waitForInitialized(page.request)
  await page.goto(`/login?redirect=${encodeURIComponent(destination)}`)
  await page.getByLabel('用户名', { exact: true }).fill(testUsername)
  await page.getByLabel('密码', { exact: true }).fill(testPassword)
  await page.getByRole('button', { name: '登录', exact: true }).click()
  await expect(page).toHaveURL(destination)
  await expect(page).toHaveTitle('Senlun')
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
}

export async function login(page: Page, destination = '/'): Promise<void> {
  const cookies = await ensureSessionCookies(page)
  await page.context().addCookies(cookies)
  await page.goto(destination)
  await expect(page.locator('#main-content')).toBeVisible()
  await expect(page).toHaveURL(destination)
  await expect(page).toHaveTitle('Senlun')
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
}

export async function mutation(request: APIRequestContext, path: string, data: unknown = {}) {
  const statusResponse = await request.get('/api/auth/status')
  expect(statusResponse.ok()).toBe(true)
  const status = await statusResponse.json()
  return request.post(path, {
    headers: { 'Origin': baseOrigin, 'X-CSRF-Token': status.csrf_token },
    data,
  })
}

export async function waitForNode(request: APIRequestContext): Promise<string> {
  await expect.poll(async () => {
    const response = await request.get('/api/admin/state')
    const state = await response.json()
    return state.nodes.find((node: { name: string }) => node.name === 'e2e-node')?.id ?? ''
  }, { timeout: 20_000 }).not.toBe('')
  const state = await (await request.get('/api/admin/state')).json()
  return state.nodes.find((node: { name: string }) => node.name === 'e2e-node').id
}
