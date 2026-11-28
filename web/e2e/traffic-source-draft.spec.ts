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

test('node and site drafts guard switch/refresh/same-document history, survive local traffic correction, and handle resync, concurrent refresh, and logout', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  const baseNode = initialState.nodes[0]
  const nodes = [
    {
      ...baseNode,
      id: 'guard-node-a',
      name: 'Guard Node A',
      public_remark: 'Original Remark A',
      disabled: false,
    },
    {
      ...baseNode,
      id: 'guard-node-b',
      name: 'Guard Node B',
      public_remark: 'Original Remark B',
      disabled: false,
    },
  ]
  const originalSiteName = initialState.settings.site_name
  let currentSettings = { ...initialState.settings }
  let failStateRefresh = false
  let holdNextStateGet = false
  let releaseHeldStateGet: (() => void) | null = null
  let stateGetCount = 0
  let nodeSaveMode: 'ok' | 'timeout' = 'ok'
  let nodeSaveCount = 0
  let logoutMode: 'ok' | 'fail-403' = 'fail-403'
  let signedOut = false

  await page.route('**/api/admin/state', async (route) => {
    stateGetCount++
    if (holdNextStateGet) {
      holdNextStateGet = false
      const staleSettings = { ...currentSettings }
      const staleNodes = nodes.map(item => ({ ...item }))
      await new Promise<void>((resolve) => {
        releaseHeldStateGet = resolve
      })
      await route.fulfill({
        json: {
          ...initialState,
          settings: staleSettings,
          nodes: staleNodes,
        },
      })
      return
    }
    if (failStateRefresh) {
      await route.fulfill({
        status: 503,
        json: { error: 'Synthetic state refresh failure' },
      })
      return
    }
    await route.fulfill({
      json: {
        ...initialState,
        settings: currentSettings,
        nodes,
      },
    })
  })
  await page.route('**/api/admin/nodes/*/traffic-correction', async (route) => {
    if (route.request().method() === 'GET') {
      await route.fulfill({
        json: {
          baseline: {
            cycle_start_ms: Date.now() - 3_600_000,
            traffic_reset_day: 1,
            traffic_limit_type: 'sum',
            used_up_bytes: '1073741824',
            used_down_bytes: '2147483648',
          },
          sampled_at_ms: Date.now(),
          traffic_limit_bytes: '0',
        },
      })
      return
    }
    nodes[0] = {
      ...nodes[0],
      selected_traffic: {
        source: 'local',
        status: 'ready',
        used_bytes: 5 * 1024 ** 3,
        limit_bytes: null,
        up_bytes: 2 * 1024 ** 3,
        down_bytes: 3 * 1024 ** 3,
        percentage: null,
        last_success_unix_ms: Date.now(),
        reset_at: null,
      },
    }
    await route.fulfill({
      json: { ok: true },
    })
  })
  await page.route('**/api/admin/nodes/guard-node-a', async (route) => {
    nodeSaveCount++
    if (nodeSaveMode === 'timeout') {
      await route.abort('timedout')
      return
    }
    const body = route.request().postDataJSON()
    nodes[0] = {
      ...nodes[0],
      ...body,
    }
    await route.fulfill({
      json: { ok: true },
    })
  })
  await page.route('**/api/admin/settings', async (route) => {
    currentSettings = {
      ...currentSettings,
      ...route.request().postDataJSON(),
    }
    await route.fulfill({
      json: { ok: true },
    })
  })
  await page.route('**/api/auth/logout', async (route) => {
    if (logoutMode === 'fail-403') {
      await route.fulfill({
        status: 403,
        json: { error: 'Origin mismatch' },
      })
      return
    }
    signedOut = true
    await route.fulfill({
      json: { ok: true },
    })
  })
  await page.route('**/api/auth/status', async (route) => {
    await route.fulfill({
      json: {
        initialized: true,
        logged_in: !signedOut,
        username: signedOut
          ? null
          : 'admin',
        csrf_token: 'synthetic-csrf-token',
        totp_enabled: false,
        oauth_enabled: false,
        oauth_totp_required: false,
      },
    })
  })

  // Seed same-document SPA history via real admin navigation before testing history.back().
  await page.goto('/admin?section=site')
  const navigation = page.getByRole('navigation', { name: '管理页面' })
  const siteLink = navigation.getByRole('link', { name: '站点设置', exact: true })
  const nodesLink = navigation.getByRole('link', { name: '节点', exact: true })
  const securityLink = navigation.getByRole('link', { name: '账号安全', exact: true })
  const topRefresh = page.getByRole('button', { name: '刷新', exact: true })
  const confirmation = page.getByRole('dialog', { name: '放弃未保存修改？' })
  const siteName = page.getByLabel('站点名称')
  await expect(siteName).toHaveValue(originalSiteName)

  await nodesLink.click()
  await expect(page).toHaveURL('/admin?section=nodes')
  const isDesktop = await page.evaluate(() => innerWidth >= 1024)
  if (!isDesktop) {
    await page.getByRole('button', { name: /Guard Node A/ }).click()
    await expect(page).toHaveURL('/admin?section=nodes&node=guard-node-a')
  }
  const remarkInput = page.getByLabel('公开备注')

  await expect(remarkInput).toHaveValue('Original Remark A')
  await remarkInput.fill('Unsaved Remark Draft')
  await expect(page.getByRole('status').filter({ hasText: '有未保存修改' })).toBeVisible()

  // Node switch (desktop) or back-to-list (mobile), top refresh, and same-document browser Back guard drafts and restore focus on cancel.
  if (isDesktop) {
    const nodeBButton = page.getByRole('button', { name: /Guard Node B/ })
    await nodeBButton.click()
    await expect(confirmation).toContainText('Guard Node B')
    await confirmation.getByRole('button', { name: '取消，继续编辑' }).click()
    await expect(nodeBButton).toBeFocused()
    await expect(remarkInput).toHaveValue('Unsaved Remark Draft')
  }
  else {
    const backToListButton = page.getByRole('button', { name: '返回节点列表' })
    await backToListButton.click()
    await expect(confirmation).toBeVisible()
    await confirmation.getByRole('button', { name: '取消，继续编辑' }).click()
    await expect(backToListButton).toBeFocused()
    await expect(remarkInput).toHaveValue('Unsaved Remark Draft')
  }

  await topRefresh.click()
  await expect(confirmation).toContainText('刷新管理数据')
  await confirmation.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(topRefresh).toBeFocused()
  await expect(remarkInput).toHaveValue('Unsaved Remark Draft')

  await page.evaluate(() => history.back())
  await expect(confirmation).toBeVisible()
  await confirmation.getByRole('button', { name: '取消，继续编辑' }).click()
  const expectedNodesUrl = isDesktop
    ? '/admin?section=nodes'
    : '/admin?section=nodes&node=guard-node-a'
  await expect(page).toHaveURL(expectedNodesUrl)
  await expect(remarkInput).toHaveValue('Unsaved Remark Draft')

  // Performing local traffic correction refreshes server state without overwriting dirty node remarks.
  await page.getByRole('button', { name: '校正本周期本地流量' }).click()
  const correctionDialog = page.getByRole('dialog', { name: '校正本周期本地流量' })
  await expect(correctionDialog.getByLabel('校正原因')).toBeVisible()
  await correctionDialog.getByLabel('目标上传已用量').fill('2')
  await correctionDialog.getByLabel('目标下载已用量').fill('3')
  await correctionDialog.getByLabel('校正原因').fill('Synthetic correction')
  await correctionDialog.getByRole('button', { name: '预览校正' }).click()
  await correctionDialog.getByRole('button', { name: '确认校正本地流量' }).click()
  await expect(correctionDialog).toBeHidden()
  await expect(remarkInput).toHaveValue('Unsaved Remark Draft')
  await expect(page.getByRole('status').filter({ hasText: '有未保存修改' })).toBeVisible()

  // Confirmed write + refresh failure must render one resync button, preserve committed guidance on failed resync, and avoid replaying POST.
  failStateRefresh = true
  await page.getByRole('button', { name: '保存节点' }).click()
  const resyncButtons = page.getByRole('button', { name: '重新同步', exact: true })
  await expect(resyncButtons).toHaveCount(1)
  await expect(page.getByRole('alert')).toContainText('操作已提交成功')
  await expect(page.getByRole('status').filter({ hasText: '有未保存修改' })).toBeHidden()
  expect(nodeSaveCount).toBe(1)

  await resyncButtons.click()
  await expect(page.getByRole('alert')).toContainText('操作已提交成功')
  await expect(page.getByRole('alert')).toContainText('Synthetic state refresh failure')
  expect(nodeSaveCount).toBe(1)

  failStateRefresh = false
  await resyncButtons.click()
  await expect(page.getByRole('status').filter({ hasText: '已重新同步最新状态' })).toBeVisible()
  expect(nodeSaveCount).toBe(1)

  // Timeout write keeps draft and surfaces uncertain resync guidance; explicit discard-and-refresh resets draft.
  await remarkInput.fill('Timed Out Remark')
  nodeSaveMode = 'timeout'
  await page.getByRole('button', { name: '保存节点' }).click()
  await expect(page.getByRole('alert')).toContainText('无法确认写入是否已生效')
  await expect(remarkInput).toHaveValue('Timed Out Remark')
  await expect(resyncButtons).toHaveCount(1)
  await topRefresh.click()
  await confirmation.getByRole('button', { name: '放弃修改并刷新' }).click()
  await expect(remarkInput).toHaveValue('Unsaved Remark Draft')
  await expect(page.getByRole('status').filter({ hasText: '有未保存修改' })).toBeHidden()

  // Dirty node -> confirm leave to site -> top refresh must only open a single site dialog (no ghost node callback).
  await remarkInput.fill('Dirty Before Leaving Nodes')
  await siteLink.click()
  await expect(confirmation).toHaveCount(1)
  await confirmation.getByRole('button', { name: '放弃修改并离开' }).click()
  await expect(page).toHaveURL('/admin?section=site')

  await siteName.fill('Unsaved Site Name')
  await topRefresh.click()
  await expect(confirmation).toHaveCount(1)
  await expect(confirmation).toContainText('刷新管理数据会丢弃当前站点设置')
  await confirmation.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(topRefresh).toBeFocused()
  await expect(siteName).toHaveValue('Unsaved Site Name')

  // Leaving site settings after confirming discard must reset site settings baseline when returning.
  await nodesLink.click()
  await expect(confirmation).toContainText('离开站点设置栏目')
  await confirmation.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(nodesLink).toBeFocused()
  await expect(siteName).toHaveValue('Unsaved Site Name')

  await nodesLink.click()
  await confirmation.getByRole('button', { name: '放弃修改并离开' }).click()
  await expect(page).toHaveURL('/admin?section=nodes')
  await siteLink.click()
  await expect(page).toHaveURL('/admin?section=site')
  await expect(siteName).toHaveValue(originalSiteName)
  await expect(page.getByRole('status').filter({ hasText: '有未保存修改' })).toBeHidden()

  // Switching to a section with no draft registrant allows top refresh without ghost prompts.
  await securityLink.click()
  await expect(page).toHaveURL('/admin?section=security')
  const getsBeforeSecurityRefresh = stateGetCount
  await topRefresh.click()
  await expect(confirmation).toHaveCount(0)
  await expect.poll(() => stateGetCount).toBeGreaterThan(getsBeforeSecurityRefresh)

  // Post-write reload during an in-flight slow GET must run a fresh post-POST GET and not revert to stale data.
  await siteLink.click()
  await expect(siteName).toHaveValue(originalSiteName)
  holdNextStateGet = true
  const getsBeforeConcurrentSave = stateGetCount
  await topRefresh.click()
  await expect.poll(() => releaseHeldStateGet !== null).toBe(true)
  await siteName.fill('Concurrent Fresh Site Name')
  await page.getByRole('button', { name: '保存设置' }).click()
  await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible()
  releaseHeldStateGet?.()
  releaseHeldStateGet = null
  await expect.poll(() => stateGetCount).toBeGreaterThanOrEqual(getsBeforeConcurrentSave + 2)
  await expect(topRefresh).toBeEnabled()
  await expect(siteName).toHaveValue('Concurrent Fresh Site Name')

  // Failed 403 logout keeps dirty site draft; successful logout leaves without prompt.
  await siteName.fill('Unsaved Before Logout')
  await page.getByRole('button', { name: '退出登录' }).click()
  await expect(page.getByRole('alert')).toContainText('Origin mismatch')
  await expect(page).toHaveURL('/admin?section=site')
  await expect(siteName).toHaveValue('Unsaved Before Logout')

  logoutMode = 'ok'
  await page.getByRole('button', { name: '退出登录' }).click()
  await expect(page).toHaveURL('/login')
  await expect(confirmation).toBeHidden()
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})

test('high-risk node uncertain locks persist across section remounts and distinguish verifiable vs unverifiable outcomes', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  const baseNode = initialState.nodes[0]
  let nodes = [
    {
      ...baseNode,
      id: 'lock-node-a',
      name: 'Lock Node A',
      disabled: false,
    },
    {
      ...baseNode,
      id: 'lock-node-b',
      name: 'Lock Node B',
      disabled: false,
    },
  ]
  let enrollMode: 'timeout' | 'ok' = 'timeout'
  let rotateMode: '502' | 'ok' = '502'

  await page.route('**/api/admin/state', route => route.fulfill({
    json: {
      ...initialState,
      nodes,
    },
  }))
  await page.route('**/api/admin/enrollment', async (route) => {
    if (enrollMode === 'timeout') {
      await route.abort('timedout')
      return
    }
    await route.fulfill({
      json: { token: 'synthetic-enroll-token' },
    })
  })
  await page.route('**/api/admin/nodes/*/rotate', async (route) => {
    if (rotateMode === '502') {
      await route.fulfill({
        status: 502,
        json: { error: 'Synthetic upstream 502' },
      })
      return
    }
    await route.fulfill({
      json: { agent_token: 'synthetic-rotated-token' },
    })
  })
  await page.route('**/api/admin/nodes/*/delete', route => route.abort('timedout'))

  await page.goto('/admin?section=nodes')
  const isDesktop = await page.evaluate(() => innerWidth >= 1024)
  const navigation = page.getByRole('navigation', { name: '管理页面' })
  const siteLink = navigation.getByRole('link', { name: '站点设置', exact: true })
  const nodesLink = navigation.getByRole('link', { name: '节点', exact: true })

  // Enrollment starts collapsed when nodes exist; timeout lock auto-expands and persists across section remounts.
  const toggleEnrollButton = page.getByRole('button', { name: /展开接入节点|收起接入节点/ })
  await expect(toggleEnrollButton).toHaveAttribute('aria-expanded', 'false')
  await toggleEnrollButton.click()
  const enrollButton = page.getByRole('button', { name: '生成一次性注册令牌' })
  await enrollButton.click()
  await expect(enrollButton).toBeDisabled()
  await expect(page.getByRole('alert')).toContainText('已暂停直接重试')
  await expect(page.getByRole('alert')).not.toContainText('请先重新同步')

  await siteLink.click()
  await expect(page).toHaveURL('/admin?section=site')
  await nodesLink.click()
  await expect(page).toHaveURL('/admin?section=nodes')
  await expect(enrollButton).toBeDisabled()
  await expect(page.getByRole('alert')).toContainText('已暂停直接重试')
  await page.getByRole('button', { name: '已知悉，允许重新生成' }).click()
  await expect(enrollButton).toBeEnabled()
  enrollMode = 'ok'
  await enrollButton.click()
  await expect(page.getByLabel('新令牌（离开页面后清除）')).toHaveValue('synthetic-enroll-token')
  await page.getByRole('button', { name: '隐藏令牌' }).click()

  // Unverifiable rotate 502 persists across close/reopen, section remounts, and ordinary state refreshes.
  if (!isDesktop)
    await page.getByRole('button', { name: /Lock Node A/ }).click()
  await page.getByRole('button', { name: '轮换 Agent 凭据' }).click()
  const rotateDialog = page.getByRole('dialog', { name: '轮换 Agent 凭据' })
  const confirmRotateButton = rotateDialog.getByRole('button', { name: '确认轮换凭据' })
  await confirmRotateButton.click()
  await expect(rotateDialog.getByRole('alert')).toContainText('Synthetic upstream 502')
  await expect(confirmRotateButton).toBeDisabled()
  await rotateDialog.getByRole('button', { name: '取消' }).click()

  await siteLink.click()
  await expect(page).toHaveURL('/admin?section=site')
  await nodesLink.click()
  await expect(page).toHaveURL('/admin?section=nodes')
  if (!isDesktop)
    await page.getByRole('button', { name: /Lock Node A/ }).click()
  await expect(page.getByRole('alert')).toContainText('轮换 Agent 凭据结果未确认')

  await page.getByRole('button', { name: '轮换 Agent 凭据' }).click()
  await expect(confirmRotateButton).toBeDisabled()
  await rotateDialog.getByRole('button', { name: '刷新管理状态' }).click()
  await expect(rotateDialog).toBeVisible()
  await expect(confirmRotateButton).toBeDisabled()
  await expect(rotateDialog.getByRole('alert')).toContainText('无法证明节点「Lock Node A」的凭据轮换是否已生效')
  await rotateDialog.getByRole('button', { name: '我已核对状态，解除锁定' }).click()
  await expect(confirmRotateButton).toBeEnabled()
  rotateMode = 'ok'
  await confirmRotateButton.click()
  await expect(rotateDialog).toBeHidden()
  await expect(page.getByLabel('新令牌（离开页面后清除）')).toHaveValue('synthetic-rotated-token')

  // Verifiable delete timeout persists across section remounts and clears once fresh GET observes deletion.
  await page.getByRole('button', { name: '删除节点' }).click()
  const deleteDialog = page.getByRole('dialog', { name: '删除节点' })
  const confirmDeleteButton = deleteDialog.getByRole('button', { name: '确认删除节点' })
  await confirmDeleteButton.click()
  await expect(confirmDeleteButton).toBeDisabled()
  await deleteDialog.getByRole('button', { name: '取消' }).click()

  await siteLink.click()
  await nodesLink.click()
  if (!isDesktop)
    await page.getByRole('button', { name: /Lock Node A/ }).click()
  await page.getByRole('button', { name: '删除节点' }).click()
  await expect(confirmDeleteButton).toBeDisabled()
  nodes = nodes.filter(item => item.id !== 'lock-node-a')
  await deleteDialog.getByRole('button', { name: '重新同步' }).click()
  await expect(deleteDialog).toBeHidden()
  if (isDesktop)
    await expect(page.getByRole('status').filter({ hasText: '已核实：节点「Lock Node A」已删除' })).toBeVisible()
  else
    await expect(page.getByRole('button', { name: /Lock Node A/ })).toHaveCount(0)
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})

test('external traffic removal persists uncertain locks across remounts and resync preserves edits made after timeout', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const state = await (await page.request.get('/api/admin/state')).json()
  const nodes = [
    {
      ...state.nodes[0],
      id: 'rm-node-a',
      name: 'Remove Node A',
    },
  ]
  const source: ExternalTrafficSource = {
    node_id: 'rm-node-a',
    config: {
      name: 'Provider A',
      enabled: false,
      auth_mode: 'bearer',
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
    },
    has_credential: false,
    snapshot: null,
    last_attempt_unix_ms: 0,
    last_success_unix_ms: 0,
    status: 'pending',
  }
  let currentSources: ExternalTrafficSource[] = [source]
  let failTrafficGet = false
  let removeMode: 'fail-500' | 'timeout' | '502' = 'fail-500'

  await page.route('**/api/admin/state', route => route.fulfill({
    json: { ...state, nodes },
  }))
  await page.route('**/api/admin/traffic-sources', route => route.fulfill(failTrafficGet
    ? {
        status: 503,
        json: { error: 'Synthetic traffic get failure' },
      }
    : {
        json: {
          sources: currentSources,
          credential_key_ready: true,
        },
      }))
  await page.route('**/api/admin/traffic-sources/*', async (route) => {
    const body = route.request().postDataJSON()
    // Simulate upstream applying the write before a gateway 502 drops the response.
    currentSources = [
      {
        ...source,
        config: body.config,
        has_credential: true,
      },
    ]
    await route.fulfill({
      status: 502,
      json: { error: 'Synthetic save 502' },
    })
  })
  await page.route('**/api/admin/traffic-sources/*/delete', async (route) => {
    if (removeMode === 'timeout') {
      await route.abort('timedout')
      return
    }
    if (removeMode === '502') {
      await route.fulfill({
        status: 502,
        json: { error: 'Synthetic remove 502' },
      })
      return
    }
    await route.fulfill({
      status: 500,
      json: { error: 'Synthetic remove failure' },
    })
  })

  await page.goto('/admin?section=traffic')
  const navigation = page.getByRole('navigation', { name: '管理页面' })
  const nodesLink = navigation.getByRole('link', { name: '节点', exact: true })
  const trafficLink = navigation.getByRole('link', { name: '外部流量', exact: true })
  const nameInput = page.getByLabel('来源名称（必填）')
  const tokenInput = page.getByLabel('Bearer Token（留空保留已存凭据）')
  await expect(nameInput).toHaveValue('Provider A')

  // Dirty external traffic route leave must trigger only a single discard dialog.
  await nameInput.fill('Dirty Provider A')
  await nodesLink.click()
  const leaveDialogs = page.getByRole('dialog', { name: '放弃未保存修改？' })
  await expect(leaveDialogs).toHaveCount(1)
  await leaveDialogs.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(leaveDialogs).toHaveCount(0)

  // Save 502 -> user edits new name + new token -> resync confirms prior save WITHOUT clearing newer inputs or token.
  await tokenInput.fill('initial-synthetic-token')
  await page.getByRole('button', { name: '保存外部流量源' }).click()
  await expect(page.getByRole('alert')).toContainText('Synthetic save 502')
  await nameInput.fill('Edited After Save 502')
  await tokenInput.fill('newer-synthetic-token')
  await page.getByRole('button', { name: '重新同步', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: '您之后的新修改与凭据输入已保留' })).toBeVisible()
  await expect(nameInput).toHaveValue('Edited After Save 502')
  await expect(tokenInput).toHaveValue('newer-synthetic-token')

  // Discard edits to return to clean baseline before verifying removal dialog and cross-section remount lock.
  await nodesLink.click()
  await leaveDialogs.getByRole('button', { name: '放弃修改并离开' }).click()
  await trafficLink.click()
  await expect(nameInput).toHaveValue('Dirty Provider A')

  // Removal confirmation names target node, keeps 500 error in dialog, and clears stale 500 error on reopen.
  await page.getByRole('button', { name: '移除流量源' }).click()
  const removeDialog = page.getByRole('dialog', { name: '移除外部流量源？' })
  await expect(removeDialog).toContainText('Remove Node A')
  await removeDialog.getByRole('button', { name: '确认移除' }).click()
  await expect(removeDialog).toBeVisible()
  await expect(removeDialog.getByRole('alert')).toContainText('Synthetic remove failure')
  await removeDialog.getByRole('button', { name: '取消' }).click()
  await expect(removeDialog).toBeHidden()

  await page.getByRole('button', { name: '移除流量源' }).click()
  await expect(removeDialog.getByRole('alert')).toHaveCount(0)

  // Timeout lock persists across close/reopen and across section remounts; failed resync preserves alert.
  removeMode = 'timeout'
  await removeDialog.getByRole('button', { name: '确认移除' }).click()
  await expect(removeDialog.getByRole('button', { name: '确认移除' })).toBeDisabled()
  await removeDialog.getByRole('button', { name: '取消' }).click()

  await nodesLink.click()
  await expect(page).toHaveURL('/admin?section=nodes')
  await trafficLink.click()
  await expect(page).toHaveURL('/admin?section=traffic')
  await expect(page.getByRole('alert')).toContainText('移除节点「Remove Node A」外部流量源结果未确认')

  await page.getByRole('button', { name: '移除流量源' }).click()
  await expect(removeDialog.getByRole('button', { name: '确认移除' })).toBeDisabled()
  failTrafficGet = true
  await removeDialog.getByRole('button', { name: '重新同步' }).click()
  await expect(removeDialog.getByRole('alert')).toContainText('勿直接重复提交')
  await expect(removeDialog.getByRole('alert')).toContainText('Synthetic traffic get failure')
  await expect(removeDialog.getByRole('button', { name: '确认移除' })).toBeDisabled()

  failTrafficGet = false
  await removeDialog.getByRole('button', { name: '重新同步' }).click()
  await expect(removeDialog).toBeVisible()
  await expect(removeDialog.getByRole('status')).toContainText('外部流量源仍存在')
  await expect(removeDialog.getByRole('button', { name: '确认移除' })).toBeEnabled()

  // Remove 502 -> close dialog -> edit new draft + token -> server removes old source -> resync verifies removal WITHOUT wiping newer draft/token.
  removeMode = '502'
  await removeDialog.getByRole('button', { name: '确认移除' }).click()
  await expect(removeDialog.getByRole('alert')).toContainText('Synthetic remove 502')
  await removeDialog.getByRole('button', { name: '取消' }).click()
  await nameInput.fill('New Draft After Remove 502')
  await tokenInput.fill('token-after-remove-502')
  currentSources = []
  await page.getByRole('button', { name: '重新同步', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: '已核实：已移除节点「Remove Node A」的外部流量源；当前未保存的草稿与凭据输入已保留。' })).toBeVisible()
  await expect(nameInput).toHaveValue('New Draft After Remove 502')
  await expect(tokenInput).toHaveValue('token-after-remove-502')
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})

test('busy clean enrollment blocks section switches, allows logout, and ignores late 502 responses after re-login', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  let releaseEnroll: (() => void) | null = null
  let enrollAttempts = 0
  let signedOut = false

  await page.route('**/api/admin/state', route => route.fulfill({
    json: initialState,
  }))
  await page.route('**/api/admin/enrollment', async (route) => {
    enrollAttempts++
    await new Promise<void>((resolve) => {
      releaseEnroll = resolve
    })
    await route.fulfill({
      status: 502,
      json: { error: 'Late stale-session 502' },
    })
  })
  await page.route('**/api/auth/logout', async (route) => {
    signedOut = true
    await route.fulfill({
      json: { ok: true },
    })
  })
  await page.route('**/api/auth/login', async (route) => {
    signedOut = false
    await route.fulfill({
      json: {
        initialized: true,
        logged_in: true,
        username: 'admin',
        csrf_token: 'relogged-in-csrf',
        totp_enabled: false,
        oauth_enabled: false,
        oauth_totp_required: false,
      },
    })
  })
  await page.route('**/api/auth/status', async (route) => {
    await route.fulfill({
      json: {
        initialized: true,
        logged_in: !signedOut,
        username: signedOut
          ? null
          : 'admin',
        csrf_token: signedOut
          ? 'anon-csrf'
          : 'active-csrf',
        totp_enabled: false,
        oauth_enabled: false,
        oauth_totp_required: false,
      },
    })
  })

  await page.goto('/admin?section=nodes')
  const navigation = page.getByRole('navigation', { name: '管理页面' })
  const siteLink = navigation.getByRole('link', { name: '站点设置', exact: true })
  await page.getByRole('button', { name: '展开接入节点' }).click()
  const enrollButton = page.getByRole('button', { name: '生成一次性注册令牌' })
  await expect(enrollButton).toBeEnabled()

  // Clean form with in-flight enrollment must block switching sections in-app so users cannot remount and double-issue.
  await enrollButton.click()
  await expect.poll(() => releaseEnroll !== null).toBe(true)
  await siteLink.click()
  await expect(page).toHaveURL('/admin?section=nodes')
  await expect(page.getByRole('button', { name: '正在生成…' })).toBeDisabled()
  expect(enrollAttempts).toBe(1)

  // Real logout must still exit cleanly while enrollment is pending; late 502 settling after re-login must NOT pollute the new session.
  await page.getByRole('button', { name: '退出登录' }).click()
  await expect(page).toHaveURL('/login')

  // Re-authenticate inside the same SPA document so shared module state stays in memory with a new session generation.
  await page.getByLabel('用户名', { exact: true }).fill('admin')
  await page.getByLabel('密码', { exact: true }).fill('synthetic-password')
  await page.getByRole('button', { name: '登录', exact: true }).click()
  await expect(page).toHaveURL('/admin')
  const expandAfterLogin = page.getByRole('button', { name: '展开接入节点' })
  await expect(expandAfterLogin).toHaveAttribute('aria-expanded', 'false')

  const lateEnrollSettled = page.waitForResponse(response => response.url().endsWith('/api/admin/enrollment') && response.status() === 502)
  releaseEnroll?.()
  releaseEnroll = null
  await lateEnrollSettled
  await expandAfterLogin.click()
  await expect(page.getByRole('button', { name: '生成一次性注册令牌' })).toBeEnabled()
  await expect(page.getByRole('alert')).toHaveCount(0)
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})
