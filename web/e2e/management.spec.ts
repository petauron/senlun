import type { TrafficCorrectionContext } from '../src/utils/trafficCorrection'
import { expect, test } from '@playwright/test'
import { nodeMetadata } from '../src/utils/admin'
import { login, mutation, waitForNode } from './auth'
import { testPassword, testUsername } from './credentials.mjs'

test('private monitoring, setup lockout, administrator CSRF, and logout enforce the session boundary', async ({ page, request }, testInfo) => {
  const anonymous = await request.get('/api/auth/status')
  const status = await anonymous.json()
  await expect.poll(async () => (await (await request.get('/api/auth/status')).json()).initialized).toBe(true)
  expect(status.logged_in).toBe(false)
  expect((await request.get('/api/v1/nodes')).status()).toBe(401)
  await page.goto('/admin')
  await expect(page).toHaveURL(/\/login\?redirect=/)

  await login(page, '/admin')
  await expect(page.getByRole('heading', { name: '管理 Senulun' })).toBeVisible()
  const screenshot = testInfo.outputPath('authenticated-admin.png')
  await page.screenshot({ path: screenshot, fullPage: true })
  await testInfo.attach('authenticated-admin', { path: screenshot, contentType: 'image/png' })
  const replay = await mutation(page.request, '/api/auth/setup', { username: testUsername, password: testPassword })
  expect(replay.status()).toBe(409)
  const state = await (await page.request.get('/api/admin/state')).json()
  const rejected = await page.request.post('/api/admin/settings', { headers: { Origin: 'http://127.0.0.1:18080' }, data: state.settings })
  expect(rejected.status()).toBe(403)
  await page.getByRole('button', { name: '退出登录' }).click()
  await expect(page).toHaveURL('/login')
  expect((await page.request.get('/api/admin/state')).status()).toBe(401)
  await expect(page.getByText('e2e-node', { exact: true })).toHaveCount(0)
})

test('public mode still hides hidden nodes and never exposes administrator writes', async ({ page, request }) => {
  await login(page, '/admin?section=site')
  const id = await waitForNode(page.request)
  const original = await (await page.request.get('/api/admin/state')).json()
  try {
    await page.getByLabel('私有站点：登录后才能查看监控数据').uncheck()
    await page.getByRole('button', { name: '保存设置' }).click()
    await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible()
    expect((await request.get('/api/v1/nodes')).status()).toBe(200)

    await page.goto(`/admin?section=nodes&node=${id}`)
    await page.getByLabel('从监控视图隐藏（管理列表保留）').check()
    await page.getByRole('button', { name: '保存节点' }).click()
    await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible()
    const dashboard = await request.post('/api/rpc2', { data: { jsonrpc: '2.0', id: 1, method: 'common:getDashboard', params: {} } })
    expect(dashboard.ok()).toBe(true)
    expect((await dashboard.json()).result.clients).not.toHaveProperty(id)
    expect((await request.get(`/api/v1/nodes/${id}/probes?hours=24`)).status()).toBe(404)
    expect((await request.get('/api/admin/state')).status()).toBe(401)
    expect((await mutation(request, '/api/admin/settings', original.settings)).status()).toBe(401)
    await expect(page.getByText('此节点已从监控页面和探测历史中隐藏。取消隐藏并保存后可查看。')).toBeVisible()
  }
  finally {
    const node = nodeMetadata(original.nodes.find((item: { id: string }) => item.id === id))
    await mutation(page.request, `/api/admin/nodes/${id}`, node)
    await mutation(page.request, '/api/admin/settings', original.settings)
  }
})

test('administrator can switch the Emerald header to the bundled map', async ({ page }) => {
  await login(page, '/admin?section=site')
  const original = await (await page.request.get('/api/admin/state')).json()
  try {
    await page.getByLabel('首页头部展示模式').selectOption('maps')
    await page.getByRole('button', { name: '保存设置' }).click()
    await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible()
    const updated = await (await page.request.get('/api/admin/state')).json()
    expect(updated.settings.earth_view_mode).toBe('maps')
    await page.goto('/')
    await expect(page.getByTestId('earth-map').locator('canvas')).toBeVisible()
  }
  finally {
    await mutation(page.request, '/api/admin/settings', original.settings)
  }
})

test('probe CRUD executes on the local Agent and exposes real result history', async ({ page }, testInfo) => {
  test.setTimeout(90_000)
  await login(page, '/admin?section=probes')
  const id = await waitForNode(page.request)
  await page.getByRole('button', { name: '刷新', exact: true }).first().click()
  const name = `e2e-health-${testInfo.project.name}-${testInfo.retry}`
  let taskId = ''
  try {
    await page.getByLabel('名称', { exact: true }).fill(name)
    await page.getByLabel('显示顺序', { exact: true }).fill('-1')
    await page.getByRole('combobox', { name: '类型', exact: true }).selectOption('http')
    await page.getByLabel('目标', { exact: true }).fill('http://127.0.0.1:18080/healthz')
    await page.getByLabel('间隔（秒）', { exact: true }).fill('5')
    await page.getByLabel('超时（秒）', { exact: true }).fill('2')
    await page.getByLabel('e2e-node', { exact: true }).check()
    const saved = page.waitForResponse(response => response.url().endsWith('/api/admin/probes') && response.request().method() === 'POST')
    await page.getByRole('button', { name: '保存任务' }).click()
    expect((await saved).ok()).toBe(true)
    const state = await (await page.request.get('/api/admin/state')).json()
    taskId = state.probes.find((task: { name: string }) => task.name === name).id
    expect(state.probes.find((task: { id: string }) => task.id === taskId).weight).toBe(-1)
    await expect(page.getByRole('listitem').filter({ hasText: name })).toBeVisible()
    await expect.poll(async () => {
      const response = await page.request.get(`/api/v1/nodes/${id}/probes?hours=1`)
      return (await response.json()).records.some((record: { task_id: string, success: boolean, latency_ms: number }) => record.task_id === taskId && record.success && record.latency_ms >= 0)
    }, { timeout: 60_000 }).toBe(true)
    await page.goto('/')
    const preview = page.getByLabel(/^最近 .+的平均延迟与探测结果，每格约 \d+ 分钟$/)
    await expect(preview).toContainText(name)
    await expect(preview).toContainText('ms')
    await expect(preview).toContainText('失败')
    const latencyPanel = page.getByRole('button', { name: `e2e-node ${name} 延迟` })
    await expect(latencyPanel).toHaveAttribute('title', new RegExp(`${name} · 最近 1 小时平均延迟 \\d+ ms`))
    await expect(page.getByRole('button', { name: `e2e-node ${name} 失败率` })).toHaveAttribute('title', new RegExp(`${name} · 最近 1 小时平均失败率 [\\d.]+%`))
    const latencySample = latencyPanel.locator('[data-slot="data-tooltip"]').last()
    await latencySample.dispatchEvent('pointerenter')
    await expect(latencySample.getByRole('tooltip', { includeHidden: true })).toContainText(new RegExp(`${name}\\s+\\d{2}:\\d{2}:\\d{2}\\s+\\d+ ms`))
    await expect(latencySample.getByRole('tooltip', { includeHidden: true })).not.toContainText('次成功探测')
    await latencyPanel.focus()
    await latencyPanel.press('Enter')
    await expect(page.getByRole('dialog', { name: 'e2e-node 延迟 / 失败率' })).toBeVisible()
    await page.keyboard.press('Escape')
    await page.getByRole('button', { name: `e2e-node ${name} 失败率` }).click()
    await expect(page.getByRole('dialog', { name: 'e2e-node 延迟 / 失败率' })).toBeVisible()
    await page.keyboard.press('Escape')
    await page.getByRole('button', { name: '列表视图' }).click()
    const listProbe = page.getByRole('button', { name: 'e2e-node 延迟 / 失败率详情' })
    await expect(listProbe.locator('[data-probe-metric="latency"] > *')).toHaveCount(10)
    await expect(listProbe.locator('[data-probe-metric="loss"] > *')).toHaveCount(10)
    await listProbe.click()
    await expect(page.getByRole('dialog', { name: 'e2e-node 延迟 / 失败率' })).toBeVisible()
    await page.keyboard.press('Escape')
    await page.goto(`/instance/${id}`)
    await expect(page.getByLabel('探测历史时间范围').getByRole('tab', { name: '1 小时', exact: true })).toHaveAttribute('data-state', 'active')
    const overview = page.getByRole('button', { name: `分析 ${name}` }).locator('..').locator('..')
    await expect(overview).toContainText(name)
    await expect(overview).toContainText('ms')
    await expect(page.getByText('延迟与失败率汇总', { exact: true })).toHaveCount(0)
    await page.goto('/admin?section=probes')
    const task = page.getByRole('listitem').filter({ hasText: name })
    await task.getByRole('button', { name: '编辑' }).click()
    await page.getByLabel('名称', { exact: true }).fill(`${name}-edited`)
    await page.getByRole('button', { name: '保存任务' }).click()
    await page.getByRole('listitem').filter({ hasText: `${name}-edited` }).getByRole('button', { name: '删除' }).click()
    await page.getByRole('dialog').getByRole('button', { name: '确认删除' }).click()
    await expect(page.getByRole('listitem').filter({ hasText: `${name}-edited` })).toHaveCount(0)
    taskId = ''
  }
  finally {
    if (taskId)
      await mutation(page.request, `/api/admin/probes/${taskId}/delete`)
  }
})

test('local traffic correction previews units, requires confirmation and preserves unrelated settings', async ({ page, request }) => {
  page.setDefaultTimeout(10_000)
  await login(page, '/admin?section=nodes')
  const id = await waitForNode(page.request)
  const path = `/api/admin/nodes/${id}/traffic-correction`
  const original = await (await page.request.get(path)).json() as TrafficCorrectionContext
  const metadata = (await (await page.request.get('/api/admin/state')).json()).nodes.find((node: { id: string }) => node.id === id)
  expect((await request.get(path)).status()).toBe(401)
  expect((await request.post(path, { data: {} })).status()).toBe(401)
  await page.goto(`/admin?section=nodes&node=${id}`)
  const errors: string[] = []
  let correctionApplied = false
  page.on('pageerror', error => errors.push(error.message))
  try {
    await page.getByRole('button', { name: '校正本周期本地流量', exact: true }).click()
    const dialog = page.getByRole('dialog')
    await expect(dialog.getByText(/最后采样/)).toBeVisible()
    await dialog.getByLabel('输入单位', { exact: true }).selectOption('gb')
    await dialog.getByLabel('目标上传已用量').fill('1')
    await dialog.getByLabel('目标下载已用量').fill('2')
    await dialog.getByLabel('校正原因').fill('Synthetic E2E correction')
    await dialog.getByRole('button', { name: '预览校正', exact: true }).click()
    await expect(dialog.getByRole('region', { name: '本地流量校正预览' })).toContainText('→ 1 GB')
    await dialog.getByRole('button', { name: '返回修改' }).click()
    await dialog.getByLabel('输入单位', { exact: true }).selectOption('gib')
    await expect(dialog.getByLabel('目标上传已用量')).toHaveValue('0.931322574615478515625')
    await dialog.getByLabel('输入单位', { exact: true }).selectOption('gb')
    await expect(dialog.getByLabel('目标上传已用量')).toHaveValue('1')
    await dialog.getByRole('button', { name: '预览校正', exact: true }).click()
    const response = page.waitForResponse(result => result.url().endsWith(path) && result.request().method() === 'POST')
    await dialog.getByRole('button', { name: '确认校正本地流量' }).click()
    const applied = await response
    expect(applied.status()).toBe(200)
    correctionApplied = true
    const sent = applied.request().postDataJSON()
    expect(sent.used_up_bytes).toBe('1000000000')
    expect(sent.used_down_bytes).toBe('2000000000')
    expect((await mutation(page.request, path, sent)).status()).toBe(409)
    await expect(dialog).not.toBeVisible()
    await expect(page.getByRole('status').filter({ hasText: '已校正本周期本地流量' })).toBeVisible()
    const corrected = await (await page.request.get(path)).json() as TrafficCorrectionContext
    expect(BigInt(corrected.baseline.used_up_bytes)).toBeGreaterThanOrEqual(1000000000n)
    expect(BigInt(corrected.baseline.used_down_bytes)).toBeGreaterThanOrEqual(2000000000n)
    const updated = (await (await page.request.get('/api/admin/state')).json()).nodes.find((node: { id: string }) => node.id === id)
    expect(nodeMetadata(updated)).toEqual(nodeMetadata(metadata))
    expect(errors).toEqual([])
    await expect(page.locator('vite-error-overlay')).toHaveCount(0)
  }
  finally {
    if (correctionApplied) {
      const fresh = await (await page.request.get(path)).json() as TrafficCorrectionContext
      const restored = await mutation(page.request, path, {
        baseline: fresh.baseline,
        used_up_bytes: original.baseline.used_up_bytes,
        used_down_bytes: original.baseline.used_down_bytes,
        reason: 'Restore disposable E2E baseline',
      })
      expect(restored.status()).toBe(200)
    }
  }
})

test('notification channels require an explicit opt-in and remain disabled when created', async ({ page }, testInfo) => {
  await login(page, '/admin?section=alerts')
  const name = `e2e-disabled-${testInfo.project.name}`
  let channelId = ''
  try {
    const enabled = page.getByLabel('启用渠道并允许发送上述告警信息')
    await expect(enabled).not.toBeChecked()
    await page.getByLabel('渠道名称', { exact: true }).fill(name)
    await page.getByLabel('Webhook URL', { exact: true }).fill('https://example.invalid/senulun-e2e')
    await page.getByRole('button', { name: '保存渠道' }).click()
    await expect(page.getByRole('listitem').filter({ hasText: name })).toBeVisible()
    const state = await (await page.request.get('/api/admin/state')).json()
    const channel = state.channels.find((item: { name: string }) => item.name === name)
    channelId = channel.id
    expect(channel.enabled).toBe(false)
    await page.getByRole('listitem').filter({ hasText: name }).getByRole('button', { name: '删除' }).click()
    await page.getByRole('dialog').getByRole('button', { name: '确认删除' }).click()
    await expect(page.getByRole('listitem').filter({ hasText: name })).toHaveCount(0)
    channelId = ''
  }
  finally {
    if (channelId)
      await mutation(page.request, `/api/admin/channels/${channelId}/delete`)
  }
})
