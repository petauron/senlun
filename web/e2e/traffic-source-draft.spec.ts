import type { ExternalTrafficConfig, ExternalTrafficSource } from '../src/utils/admin'
import { expect, test } from '@playwright/test'
import { login } from './auth'

test('traffic status refresh preserves drafts and credentials until a node switch is confirmed', async ({ page }) => {
  await login(page, '/admin')
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
  await nodeSelect.selectOption(nodes[1]!.id)
  await page.getByRole('dialog', { name: '放弃未保存修改？' }).getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(nodeSelect).toBeFocused()
  await expect(name).toHaveValue('Unsaved source')
  await authMode.selectOption('login_refresh')
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

  await nodeSelect.selectOption(nodes[1]!.id)
  await expect(confirmation).toContainText('Draft node B')
  await confirmation.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(nodeSelect).toHaveValue(nodes[0]!.id)
  await expect(password).toHaveValue('synthetic-password')
  await expect(name).toHaveValue('Unsaved source')
  await nodeSelect.selectOption(nodes[1]!.id)
  await page.keyboard.press('Escape')
  await expect(confirmation).toBeHidden()
  await expect(nodeSelect).toBeFocused()
  await expect(nodeSelect).toHaveValue(nodes[0]!.id)
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
  await nodeSelect.selectOption(nodes[1]!.id)
  await expect(confirmation).toBeVisible()
  await confirmation.getByRole('button', { name: '放弃修改并切换' }).click()
  await expect(nodeSelect).toHaveValue(nodes[1]!.id)
  await expect(name).toHaveValue('')
  await expect(dirty).toBeHidden()
  await nodeSelect.selectOption(nodes[0]!.id)
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
