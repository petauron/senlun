import { expect, test } from '@playwright/test'
import { login, waitForNode } from './auth'

function buildSyntheticNodes(baseNode: Record<string, unknown>, extraCount = 0) {
  const core = [
    {
      ...baseNode,
      id: 'aa112233-0000-0000-0000-000000000001',
      name: 'Alpha SG',
      region: 'SG',
      group: 'Asia',
      hidden: false,
      disabled: false,
      traffic_source: 'local',
      public_remark: 'Primary Singapore edge',
    },
    {
      ...baseNode,
      id: 'bb445566-0000-0000-0000-000000000002',
      name: 'Beta JP',
      region: 'JP',
      group: 'Asia',
      hidden: true,
      disabled: false,
      traffic_source: 'local',
      public_remark: 'Hidden Tokyo staging',
    },
    {
      ...baseNode,
      id: 'cc778899-0000-0000-0000-000000000003',
      name: 'Gamma US',
      region: 'US',
      group: 'Americas',
      hidden: false,
      disabled: true,
      traffic_source: 'provider',
      public_remark: 'Provider metered relay',
    },
  ]
  const extras = Array.from({ length: extraCount }, (_, index) => ({
    ...baseNode,
    id: `dd${String(index + 10).padStart(6, '0')}-0000-0000-0000-000000000000`,
    name: `Scroll Node ${index + 1}`,
    region: 'EU',
    group: 'Europe',
    hidden: false,
    disabled: false,
    traffic_source: 'provider',
    public_remark: `Synthetic list item ${index + 1}`,
  }))
  return [...core, ...extras]
}

test('node management supports searchable/filterable workflows on desktop and mobile without implicit switching and with aligned controls', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  const nodes = buildSyntheticNodes(initialState.nodes[0], 14)

  await page.route('**/api/admin/state', route => route.fulfill({
    json: {
      ...initialState,
      nodes,
    },
  }))
  await page.route('**/api/admin/nodes/*', async (route) => {
    const body = route.request().postDataJSON()
    const idx = nodes.findIndex(item => item.id === body.id)
    if (idx >= 0) {
      nodes[idx] = {
        ...nodes[idx],
        ...body,
      }
    }
    await route.fulfill({
      json: { ok: true },
    })
  })

  await page.goto('/admin?section=nodes')
  const isDesktop = await page.evaluate(() => innerWidth >= 1024)

  // Enrollment card starts collapsed when nodes exist.
  const enrollToggle = page.getByRole('button', { name: '展开接入节点' })
  await expect(enrollToggle).toHaveAttribute('aria-expanded', 'false')
  await expect(page.getByRole('button', { name: '生成一次性注册令牌' })).toHaveCount(0)

  const searchInput = page.getByLabel('搜索节点')
  const filterGroup = page.getByRole('group', { name: '按节点状态筛选' })
  await expect(filterGroup.getByRole('button', { name: '全部', exact: true })).toBeVisible()
  await expect(filterGroup.getByRole('button', { name: '已隐藏', exact: true })).toBeVisible()
  await expect(filterGroup.getByRole('button', { name: '凭据已撤销', exact: true })).toBeVisible()
  await expect(filterGroup.getByRole('button', { name: '服务商流量', exact: true })).toBeVisible()

  // Search by remark and ID prefix using real ManagedNode fields only.
  await searchInput.fill('cc7788')
  const list = page.getByRole('list', { name: '可管理节点' })
  await expect(list.getByRole('button')).toHaveCount(1)
  await expect(list.getByRole('button', { name: /Gamma US/ })).toBeVisible()
  await page.getByRole('button', { name: '清空', exact: true }).click()
  await expect(searchInput).toHaveValue('')

  if (isDesktop) {
    // Desktop: filtering out the currently edited node must NOT implicitly switch editor or discard dirty input.
    const remarkInput = page.getByLabel('公开备注')
    await expect(remarkInput).toHaveValue('Primary Singapore edge')
    await remarkInput.fill('Dirty SG remark preserved across filter')

    await filterGroup.getByRole('button', { name: '已隐藏', exact: true }).click()
    await expect(list.getByRole('button', { name: /Beta JP/ })).toBeVisible()
    await expect(list.getByRole('button', { name: /Alpha SG/ })).toHaveCount(0)
    await expect(page.getByRole('heading', { name: 'Alpha SG', exact: true })).toBeVisible()
    await expect(remarkInput).toHaveValue('Dirty SG remark preserved across filter')
    await expect(page.getByRole('status').filter({ hasText: '不在当前筛选结果中' })).toBeVisible()

    await page.getByRole('button', { name: '清除筛选并显示当前节点' }).click()
    await expect(list.getByRole('button', { name: /Alpha SG/ })).toBeVisible()
    await expect(remarkInput).toHaveValue('Dirty SG remark preserved across filter')

    await page.getByRole('button', { name: '保存节点' }).click()
    await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible()
  }
  else {
    // Mobile: starts in list state without editor, enters editor on tap, and restores search, filter, scroll, and focus on return.
    await expect(page.getByRole('button', { name: '保存节点' })).toHaveCount(0)
    const providerFilter = filterGroup.getByRole('button', { name: '服务商流量', exact: true })
    await providerFilter.click()
    await expect(providerFilter).toHaveAttribute('aria-pressed', 'true')
    await searchInput.fill('Scroll Node')
    await expect(list.getByRole('button')).toHaveCount(14)

    const targetButton = list.getByRole('button', { name: /Scroll Node 12/ })
    await targetButton.scrollIntoViewIfNeeded()
    const scrollBeforeOpen = await page.evaluate(() => Math.round(window.scrollY))
    expect(scrollBeforeOpen).toBeGreaterThan(0)

    await targetButton.click()
    await expect(page).toHaveURL(/section=nodes&node=dd000021-/)
    await expect(page.getByRole('heading', { name: 'Scroll Node 12', exact: true })).toBeVisible()
    await expect(page.getByRole('button', { name: '校正本周期本地流量' })).toBeVisible()

    // Dirty edit guards returning to list and restores focus to back button on cancel.
    const remarkInput = page.getByLabel('公开备注')
    await remarkInput.fill('Updated mobile remark')
    const backButton = page.getByRole('button', { name: '返回节点列表' })
    await backButton.click()
    const discardDialog = page.getByRole('dialog', { name: '放弃未保存修改？' })
    await expect(discardDialog).toBeVisible()
    await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
    await expect(backButton).toBeFocused()

    // Save then return to list: search query, active filter, scroll position, and focus on the previously opened node button are restored.
    await page.getByRole('button', { name: '保存节点' }).click()
    await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible()
    await backButton.click()
    await expect(page).toHaveURL('/admin?section=nodes')
    await expect(searchInput).toHaveValue('Scroll Node')
    await expect(providerFilter).toHaveAttribute('aria-pressed', 'true')
    await expect(list.getByRole('button', { name: /Scroll Node 12/ })).toBeFocused()
    await expect.poll(() => page.evaluate(() => Math.round(window.scrollY))).toBeGreaterThan(0)
    const scrollAfterReturn = await page.evaluate(() => Math.round(window.scrollY))
    expect(Math.abs(scrollAfterReturn - scrollBeforeOpen)).toBeLessThanOrEqual(80)

    // Re-enter a node to inspect danger dialog copy and control heights below.
    await list.getByRole('button', { name: /Scroll Node 12/ }).click()
  }

  // Danger section has aligned >=44px controls and concrete confirmation verbs with target node name.
  const dangerSection = page.getByRole('region', { name: '危险操作' })
  const dangerButtons = dangerSection.getByRole('button')
  await expect(dangerButtons).toHaveCount(4)
  const boxes = await Promise.all([0, 1, 2, 3].map(index => dangerButtons.nth(index).boundingBox()))
  for (const box of boxes) {
    expect(box).not.toBeNull()
    expect(box!.height).toBeGreaterThanOrEqual(43)
  }
  expect(Math.abs(boxes[0]!.height - boxes[1]!.height)).toBeLessThanOrEqual(2)
  expect(Math.abs(boxes[2]!.height - boxes[3]!.height)).toBeLessThanOrEqual(2)

  await dangerSection.getByRole('button', { name: '重置本周期流量' }).click()
  const resetDialog = page.getByRole('dialog', { name: '重置本周期流量' })
  const expectedTargetName = isDesktop
    ? 'Alpha SG'
    : 'Scroll Node 12'
  await expect(resetDialog).toContainText(expectedTargetName)
  await expect(resetDialog.getByRole('button', { name: '确认重置流量' })).toBeVisible()
  await resetDialog.getByRole('button', { name: '取消' }).click()
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})

test('explicit invalid node deep link shows not-found without falling back to the first node, while valid deep link and zero-node enrollment work', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  let nodes = buildSyntheticNodes(initialState.nodes[0])

  await page.route('**/api/admin/state', route => route.fulfill({
    json: {
      ...initialState,
      nodes,
    },
  }))

  // Valid deep link lands directly on Beta JP on both desktop and mobile.
  await page.goto('/admin?section=nodes&node=bb445566-0000-0000-0000-000000000002')
  await expect(page.getByRole('heading', { name: 'Beta JP', exact: true })).toBeVisible()
  await expect(page.getByLabel('公开备注')).toHaveValue('Hidden Tokyo staging')

  // Explicit invalid deep link MUST show not-found and MUST NOT render an editable form for Alpha SG.
  await page.goto('/admin?section=nodes&node=nonexistent-node-404')
  await expect(page.getByRole('alert')).toContainText('nonexistent-node-404')
  await expect(page.getByRole('button', { name: '保存节点' })).toHaveCount(0)
  await expect(page.getByLabel('公开备注')).toHaveCount(0)

  await page.getByRole('button', { name: '返回节点列表' }).click()
  await expect(page).toHaveURL('/admin?section=nodes')
  await expect(page.getByRole('list', { name: '可管理节点' })).toBeVisible()

  // Zero-node state auto-expands enrollment panel so onboarding is immediately actionable.
  nodes = []
  await page.goto('/admin?section=nodes')
  await expect(page.getByRole('button', { name: '收起接入节点' })).toHaveAttribute('aria-expanded', 'true')
  await expect(page.getByRole('button', { name: '生成一次性注册令牌' })).toBeVisible()
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})

test('401 session expiry while saving a dirty node exits cleanly to login without a discard prompt', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  const nodes = buildSyntheticNodes(initialState.nodes[0])
  let expired = false

  await page.route('**/api/admin/state', route => route.fulfill({
    json: {
      ...initialState,
      nodes,
    },
  }))
  await page.route('**/api/admin/nodes/*', async (route) => {
    expired = true
    await route.fulfill({
      status: 401,
      json: { error: 'Session expired' },
    })
  })
  await page.route('**/api/auth/status', route => route.fulfill({
    json: {
      initialized: true,
      logged_in: !expired,
      username: expired
        ? null
        : 'admin',
      csrf_token: 'synthetic-csrf',
      totp_enabled: false,
      oauth_enabled: false,
      oauth_totp_required: false,
    },
  }))

  await page.goto('/admin?section=nodes&node=aa112233-0000-0000-0000-000000000001')
  await page.getByLabel('公开备注').fill('Dirty before 401 expiry')
  await page.getByRole('button', { name: '保存节点' }).click()
  await expect(page).toHaveURL(/\/login\?redirect=/)
  await expect(page.getByRole('dialog', { name: '放弃未保存修改？' })).toBeHidden()
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})
