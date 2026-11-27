import type { ExternalTrafficConfig, ExternalTrafficSource } from '../src/utils/admin'
import { expect, test } from '@playwright/test'
import { login, waitForNode } from './auth'

test('traffic status refresh preserves drafts and credentials until a node switch is confirmed', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const state = await (await page.request.get('/api/admin/state')).json()
  expect(state.nodes.length).toBeGreaterThan(0)
  const nodes = [
    { ...state.nodes[0], id: 'draft-node-a', name: 'Draft node A' },
    { ...state.nodes[0], id: 'draft-node-b', name: 'Draft node B' },
  ]
  const config: ExternalTrafficConfig = {
    name: 'Saved source',
    enabled: false,
    auth_mode: 'none',
    traffic_url: 'https://example.invalid/usage',
    login_url: '',
    refresh_url: '',
    username_field: 'email',
    password_field: 'password',
    refresh_token_field: 'refresh_token',
    access_pointer: '/data/access_token',
    refresh_pointer: '/data/refresh_token',
    up_pointer: '/up',
    down_pointer: '/down',
    used_pointer: '',
    limit_pointer: '',
    limit_unit: 'bytes',
    reset_pointer: '',
  }
  const source: ExternalTrafficSource = {
    node_id: nodes[0]!.id,
    config,
    has_credential: false,
    snapshot: null,
    last_attempt_unix_ms: 0,
    last_success_unix_ms: 0,
    status: 'pending',
  }
  let failRefresh = false
  let failSave = true
  let mutations = 0
  await page.route('**/api/admin/state', route => route.fulfill({
    json: { ...state, nodes },
  }))
  await page.route('**/api/admin/traffic-sources', route => route.fulfill(failRefresh
    ? {
        status: 503,
        json: { error: 'Synthetic refresh failure' },
      }
    : {
        json: {
          sources: [source],
          credential_key_ready: true,
        },
      }))
  // All traffic source mutations stay inside the test; no provider is contacted.
  await page.route('**/api/admin/traffic-sources/*', async (route) => {
    mutations++
    if (failSave) {
      await route.fulfill({
        status: 503,
        json: { error: 'Synthetic save failure' },
      })
      return
    }
    source.config = route.request().postDataJSON().config
    source.has_credential = true
    await route.fulfill({
      json: { ok: true },
    })
  })
  await page.goto('/admin?section=traffic')
  const nodeSelect = page.getByRole('combobox', { name: '关联节点' })
  const name = page.getByLabel('来源名称（必填）')
  const authMode = page.getByRole('combobox', { name: '登录方式' })
  const refresh = page.getByRole('button', { name: '刷新同步状态' })
  const dirty = page.getByRole('status').filter({ hasText: '有未保存修改' })
  const navigation = page.getByRole('navigation', { name: '管理页面' })
  const nodesLink = navigation.getByRole('link', { name: '节点', exact: true })
  const trafficLink = navigation.getByRole('link', { name: '外部流量', exact: true })
  const confirmation = page.getByRole('dialog', { name: '放弃未保存修改？' })
  await expect(name).toHaveValue('Saved source')
  // A clean form must not interrupt ordinary section navigation.
  await nodesLink.click()
  await expect(page).toHaveURL('/admin?section=nodes')
  await expect(confirmation).toBeHidden()
  await trafficLink.click()
  await expect(name).toHaveValue('Saved source')

  await name.fill('Unsaved source')
  await nodeSelect.click()
  await page.getByRole('option', { name: nodes[1]!.name, exact: true }).click()
  await page.getByRole('dialog', { name: '放弃未保存修改？' }).getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(nodeSelect).toBeFocused()
  await expect(name).toHaveValue('Unsaved source')
  await authMode.click()
  await page.getByRole('option', { name: '登录 + Refresh Token', exact: true }).click()
  await page.getByLabel('登录网址（HTTPS，POST，必填）').fill('https://example.invalid/login')
  await page.getByLabel('刷新网址（HTTPS，POST，必填）').fill('https://example.invalid/refresh')
  const username = page.getByLabel('账号（更新时与密码一同填写）')
  const password = page.getByLabel('密码（留空保留已存凭据）', { exact: true })
  await username.fill('synthetic-account')
  await password.fill('synthetic-password')
  source.status = 'provider_unavailable'
  await refresh.click()
  await expect(page.getByText('状态：服务商暂时不可用')).toBeVisible()
  await expect(name).toHaveValue('Unsaved source')
  await expect(username).toHaveValue('synthetic-account')
  await expect(password).toHaveValue('synthetic-password')
  await expect(dirty).toBeVisible()

  failRefresh = true
  await refresh.click()
  await expect(page.getByRole('alert')).toContainText('Synthetic refresh failure')
  await expect(name).toHaveValue('Unsaved source')
  await expect(password).toHaveValue('synthetic-password')
  failRefresh = false
  await refresh.click()
  await expect(refresh).toBeEnabled()
  await page.getByRole('button', { name: '保存外部流量源' }).click()
  await expect(page.getByRole('alert')).toContainText('Synthetic save failure')
  await expect(password).toHaveValue('synthetic-password')

  const writesBeforeLeaving = mutations
  await nodesLink.click()
  await expect(confirmation).toContainText('离开外部流量栏目')
  await expect(page).toHaveURL('/admin?section=traffic')
  await confirmation.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(nodesLink).toBeFocused()
  await expect(name).toHaveValue('Unsaved source')
  await expect(username).toHaveValue('synthetic-account')
  await expect(password).toHaveValue('synthetic-password')
  expect(mutations).toBe(writesBeforeLeaving)
  await nodesLink.click()
  await expect(confirmation).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(confirmation).toBeHidden()
  await expect(page).toHaveURL('/admin?section=traffic')
  await expect(password).toHaveValue('synthetic-password')
  expect(mutations).toBe(writesBeforeLeaving)
  await page.evaluate(() => history.back())
  await expect(confirmation).toBeVisible()
  await confirmation.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(page).toHaveURL('/admin?section=traffic')
  await expect(password).toHaveValue('synthetic-password')
  expect(mutations).toBe(writesBeforeLeaving)

  await nodeSelect.click()
  await page.getByRole('option', { name: nodes[1]!.name, exact: true }).click()
  await expect(confirmation).toContainText('Draft node B')
  await confirmation.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(nodeSelect).toContainText(nodes[0]!.name)
  await expect(password).toHaveValue('synthetic-password')
  await expect(name).toHaveValue('Unsaved source')
  await nodeSelect.click()
  await page.getByRole('option', { name: nodes[1]!.name, exact: true }).click()
  await page.keyboard.press('Escape')
  await expect(confirmation).toBeHidden()
  await expect(nodeSelect).toBeFocused()
  await expect(nodeSelect).toContainText(nodes[0]!.name)
  await expect(password).toHaveValue('synthetic-password')

  // A confirmed save clears secrets and establishes a clean form baseline.
  failSave = false
  await page.getByRole('button', { name: '保存外部流量源' }).click()
  await expect(page.getByRole('status').filter({ hasText: '已保存；启用后' })).toBeVisible()
  await expect(password).toHaveValue('')
  await expect(username).toHaveValue('')
  await expect(dirty).toBeHidden()
  await nodesLink.click()
  await expect(page).toHaveURL('/admin?section=nodes')
  await expect(confirmation).toBeHidden()
  await trafficLink.click()
  await expect(name).toHaveValue('Unsaved source')
  // Credentials alone must also count as unsaved changes.
  await password.fill('another-synthetic-password')
  await nodeSelect.click()
  await page.getByRole('option', { name: nodes[1]!.name, exact: true }).click()
  await expect(confirmation).toBeVisible()
  await confirmation.getByRole('button', { name: '放弃修改并切换' }).click()
  await expect(nodeSelect).toContainText(nodes[1]!.name)
  await expect(name).toHaveValue('')
  await expect(dirty).toBeHidden()
  await nodeSelect.click()
  await page.getByRole('option', { name: nodes[0]!.name, exact: true }).click()
  await expect(name).toHaveValue('Unsaved source')
  await expect(password).toHaveValue('')
  await expect(confirmation).toBeHidden()
  await password.fill('discard-only-synthetic-password')
  await nodesLink.click()
  await confirmation.getByRole('button', { name: '放弃修改并离开' }).click()
  await expect(page).toHaveURL('/admin?section=nodes')
  await expect(confirmation).toBeHidden()
  await trafficLink.click()
  await expect(name).toHaveValue('Unsaved source')
  await expect(password).toHaveValue('')
  await expect(dirty).toBeHidden()
  expect(mutations).toBe(2)
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})

test('logout and 401 expiry leave dirty traffic drafts cleanly, and expired oauth_totp recovers', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const state = await (await page.request.get('/api/admin/state')).json()
  let logoutCalls = 0
  let statusCalls = 0
  let oauthTotpRequired = false
  let statusCsrf = 'synthetic-csrf'
  let loginCsrfSeen = ''
  await page.route('**/api/admin/traffic-sources', route => route.fulfill({
    json: { sources: [], credential_key_ready: true },
  }))
  await page.route('**/api/auth/logout', async (route) => {
    logoutCalls++
    await route.fulfill({
      json: { ok: true },
    })
  })
  await page.route('**/api/auth/status', async (route) => {
    statusCalls++
    await route.fulfill({
      json: {
        initialized: true,
        logged_in: logoutCalls === 0,
        username: logoutCalls === 0
          ? 'admin'
          : null,
        csrf_token: statusCsrf,
        totp_enabled: false,
        oauth_enabled: true,
        oauth_totp_required: oauthTotpRequired,
      },
    })
  })

  await page.goto('/admin?section=traffic')
  const refreshButton = page.getByRole('button', { name: '刷新同步状态' })
  await expect(refreshButton).toBeEnabled()
  const name = page.getByLabel('来源名称（必填）')
  await name.fill('Unsaved before logout')
  await page.getByRole('button', { name: '退出登录' }).click()
  await expect(page).toHaveURL('/login')
  await expect(page.getByRole('dialog', { name: '放弃未保存修改？' })).toBeHidden()
  expect(logoutCalls).toBe(1)

  // Stale oauth_totp=required after 10-minute expiry must recover to standard login.
  await page.goto('/login?oauth_totp=required')
  await expect(page.getByLabel('用户名', { exact: true })).toBeVisible()
  await expect(page.getByLabel('密码', { exact: true })).toBeVisible()
  await expect(page.getByText('OAuth 两步验证会话已过期')).toBeVisible()

  // Manual return to standard login must refresh CSRF before allowing login submit.
  oauthTotpRequired = true
  statusCsrf = 'stale-before-manual-return'
  await page.goto('/login?oauth_totp=required')
  const returnButton = page.getByRole('button', { name: '返回常规登录' })
  await expect(returnButton).toBeEnabled()
  const callsBeforeReturn = statusCalls
  statusCsrf = 'refreshed-on-manual-return'
  await returnButton.click()
  await expect(page.getByLabel('用户名', { exact: true })).toBeVisible()
  expect(statusCalls).toBeGreaterThan(callsBeforeReturn)

  // Keeping the OAuth TOTP page open until partial-session expiry returns 403 on submit
  // and must refresh CSRF before allowing standard password login.
  oauthTotpRequired = true
  statusCsrf = 'stale-partial-oauth-csrf'
  await page.route('**/api/auth/oauth/complete', async (route) => {
    oauthTotpRequired = false
    statusCsrf = 'fresh-anon-csrf'
    await route.fulfill({
      status: 403,
      json: { error: 'Invalid CSRF token' },
    })
  })
  await page.route('**/api/auth/login', async (route) => {
    loginCsrfSeen = route.request().headers()['x-csrf-token'] ?? ''
    logoutCalls = 0
    await route.fulfill({
      json: {
        initialized: true,
        logged_in: true,
        username: 'admin',
        csrf_token: 'post-login-csrf',
        totp_enabled: false,
        oauth_enabled: true,
        oauth_totp_required: false,
      },
    })
  })
  await page.goto('/login?oauth_totp=required&redirect=%2F')
  await expect(page.getByRole('button', { name: '返回常规登录' })).toBeVisible()
  await page.getByLabel('两步验证码').fill('123456')
  await page.getByRole('button', { name: '登录', exact: true }).click()
  await expect(page.getByText('OAuth 两步验证会话已过期')).toBeVisible()
  await expect(page).toHaveURL(/\/login\?redirect=(?:\/|%2F)$/)
  await expect(page.getByLabel('用户名', { exact: true })).toBeVisible()
  await page.getByLabel('用户名', { exact: true }).fill('admin')
  await page.getByLabel('密码', { exact: true }).fill('synthetic-password')
  await page.getByRole('button', { name: '登录', exact: true }).click()
  await expect(page).toHaveURL('/')
  expect(loginCsrfSeen).toBe('fresh-anon-csrf')

  // Simulate session expiry (401) during an active save on a dirty draft.
  logoutCalls = 0
  await page.route('**/api/admin/state', route => route.fulfill({ json: state }))
  await page.route('**/api/admin/traffic-sources/*', route => route.fulfill({
    status: 401,
    json: { error: 'Session expired' },
  }))
  await page.goto('/admin?section=traffic')
  await expect(refreshButton).toBeEnabled()
  await name.fill('Unsaved before 401')
  await page.getByLabel('流量查询网址（HTTPS，GET，必填）').fill('https://example.invalid/usage')
  await page.getByLabel('上行字节路径（必填）').fill('/up')
  await page.getByLabel('下行字节路径（必填）').fill('/down')
  logoutCalls = 1
  await page.getByRole('button', { name: '保存外部流量源' }).click()
  await expect(page).toHaveURL(/\/login\?redirect=/)
  await expect(page.getByRole('dialog', { name: '放弃未保存修改？' })).toBeHidden()
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})
