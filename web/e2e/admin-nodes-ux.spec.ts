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
  return [
    ...core,
    ...extras,
  ]
}

test('node management supports searchable/filterable workflows, context restoration, aligned 44px controls, and no narrow overflow', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  const nodes = buildSyntheticNodes(initialState.nodes[0], 14)
  let failNextStateRefresh = false

  await page.route('**/api/admin/state', async (route) => {
    if (failNextStateRefresh) {
      failNextStateRefresh = false
      await route.fulfill({
        status: 503,
        json: { error: 'Synthetic save refresh failure' },
      })
      return
    }
    await route.fulfill({
      json: {
        ...initialState,
        nodes,
      },
    })
  })
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

  // Enrollment panel exists in DOM for aria-controls and starts hidden when nodes exist.
  const enrollToggle = page.getByRole('button', { name: '展开接入节点' })
  await expect(enrollToggle).toHaveAttribute('aria-expanded', 'false')
  await expect(enrollToggle).toHaveAttribute('aria-controls', 'admin-node-enroll-panel')
  await expect(page.locator('#admin-node-enroll-panel')).toHaveCount(1)
  await expect(page.getByRole('button', { name: '生成一次性注册令牌' })).toBeHidden()

  const searchInput = page.getByLabel('搜索节点')
  const filterGroup = page.getByRole('group', { name: '按节点状态筛选' })
  await expect(filterGroup.getByRole('button', { name: '全部', exact: true })).toBeVisible()
  await expect(filterGroup.getByRole('button', { name: '已隐藏', exact: true })).toBeVisible()
  await expect(filterGroup.getByRole('button', { name: '凭据已撤销', exact: true })).toBeVisible()
  await expect(filterGroup.getByRole('button', { name: '服务商流量', exact: true })).toBeVisible()

  // Clearing search must return focus to the search input instead of dropping focus to body.
  await searchInput.fill('cc7788')
  const list = page.getByRole('list', { name: '可管理节点' })
  await expect(list.getByRole('button')).toHaveCount(1)
  await expect(list.getByRole('button', { name: /Gamma US/ })).toBeVisible()
  await page.getByRole('button', { name: '清空', exact: true }).click()
  await expect(searchInput).toHaveValue('')
  await expect(searchInput).toBeFocused()

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

    // Trigger refresh failure to check Save + Resync same-row height alignment (>= 44px).
    failNextStateRefresh = true
    await page.getByRole('button', { name: '保存节点' }).click()
    const saveButton = page.getByRole('button', { name: '保存节点' })
    const resyncButton = page.getByRole('button', { name: '重新同步', exact: true })
    await expect(resyncButton).toBeVisible()
    const saveBox = await saveButton.boundingBox()
    const resyncBox = await resyncButton.boundingBox()
    expect(saveBox).not.toBeNull()
    expect(resyncBox).not.toBeNull()
    expect(saveBox!.height).toBeGreaterThanOrEqual(44)
    expect(resyncBox!.height).toBeGreaterThanOrEqual(44)
    expect(Math.abs(saveBox!.height - resyncBox!.height)).toBeLessThanOrEqual(1)
    await resyncButton.click()
    await expect(page.getByRole('status').filter({ hasText: '已重新同步最新状态' })).toBeVisible()

    // Viewing monitor (/instance/:id) and navigating back restores search, filter, and selected non-first node context.
    await filterGroup.getByRole('button', { name: '服务商流量', exact: true }).click()
    await searchInput.fill('Scroll Node 12')
    await list.getByRole('button', { name: /Scroll Node 12/ }).click()
    await expect(page.getByRole('heading', { name: 'Scroll Node 12', exact: true })).toBeVisible()
    await page.getByRole('button', { name: '查看监控' }).click()
    await expect(page).toHaveURL('/instance/dd000021-0000-0000-0000-000000000000')
    await page.goBack()
    await expect(page).toHaveURL(/section=nodes&node=dd000021-/)
    await expect(searchInput).toHaveValue('Scroll Node 12')
    await expect(filterGroup.getByRole('button', { name: '服务商流量', exact: true })).toHaveAttribute('aria-pressed', 'true')
    await expect(page.getByRole('heading', { name: 'Scroll Node 12', exact: true })).toBeVisible()
    await page.getByRole('button', { name: '清空', exact: true }).click()
    await list.getByRole('button', { name: /Alpha SG/ }).click()
    await expect(page.getByRole('heading', { name: 'Alpha SG', exact: true })).toBeVisible()
  }
  else {
    // Mobile: starts in list state without editor and without horizontal overflow.
    await expect(page.getByRole('button', { name: '保存节点' })).toHaveCount(0)
    const listOverflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)
    expect(listOverflow).toBeLessThanOrEqual(0)

    const providerFilter = filterGroup.getByRole('button', { name: '服务商流量', exact: true })
    await providerFilter.click()
    await expect(providerFilter).toHaveAttribute('aria-pressed', 'true')
    await searchInput.fill('Scroll Node')
    await expect(list.getByRole('button')).toHaveCount(14)

    const targetButton = list.getByRole('button', { name: /Scroll Node 12/ })
    await targetButton.scrollIntoViewIfNeeded()
    const scrollBeforeOpen = await page.evaluate(() => Math.round(window.scrollY))
    expect(scrollBeforeOpen).toBeGreaterThan(0)
    const historyLengthBeforeOpen = await page.evaluate(() => window.history.length)

    await targetButton.click()
    await expect(page).toHaveURL(/section=nodes&node=dd000021-/)
    await expect(page.getByRole('heading', { name: 'Scroll Node 12', exact: true })).toBeVisible()
    await expect(page.getByRole('button', { name: '校正本周期本地流量' })).toBeVisible()
    const editorOverflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)
    expect(editorOverflow).toBeLessThanOrEqual(0)

    // Dirty edit guards returning to list and restores focus to back button on cancel.
    const remarkInput = page.getByLabel('公开备注')
    await remarkInput.fill('Updated mobile remark')
    const backButton = page.getByRole('button', { name: '返回节点列表' })
    await backButton.click()
    const discardDialog = page.getByRole('dialog', { name: '放弃未保存修改？' })
    await expect(discardDialog).toBeVisible()
    await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
    await expect(backButton).toBeFocused()

    // Save with refresh failure to verify Save + Resync alignment, then resync and return to list.
    failNextStateRefresh = true
    await page.getByRole('button', { name: '保存节点' }).click()
    const saveButton = page.getByRole('button', { name: '保存节点' })
    const resyncButton = page.getByRole('button', { name: '重新同步', exact: true })
    await expect(resyncButton).toBeVisible()
    const saveBox = await saveButton.boundingBox()
    const resyncBox = await resyncButton.boundingBox()
    expect(saveBox!.height).toBeGreaterThanOrEqual(44)
    expect(resyncBox!.height).toBeGreaterThanOrEqual(44)
    expect(Math.abs(saveBox!.height - resyncBox!.height)).toBeLessThanOrEqual(1)
    await resyncButton.click()
    await expect(page.getByRole('status').filter({ hasText: '已重新同步最新状态' })).toBeVisible()

    // Explicit back uses history.back (no duplicate adjacent list entry), shows brief save feedback, highlights & focuses last node, and restores search/filter/scroll.
    await backButton.click()
    await expect(page).toHaveURL('/admin?section=nodes')
    await expect(searchInput).toHaveValue('Scroll Node')
    await expect(providerFilter).toHaveAttribute('aria-pressed', 'true')
    await expect(page.getByRole('status').filter({ hasText: '已保存节点「Scroll Node 12」' })).toBeVisible()
    const returnedButton = list.getByRole('button', { name: /Scroll Node 12/ })
    await expect(returnedButton).toBeFocused()
    await expect(returnedButton).toHaveAttribute('data-last-active', 'true')
    await expect.poll(() => page.evaluate(() => Math.round(window.scrollY))).toBeGreaterThan(0)
    const scrollAfterReturn = await page.evaluate(() => Math.round(window.scrollY))
    expect(Math.abs(scrollAfterReturn - scrollBeforeOpen)).toBeLessThanOrEqual(80)
    const historyLengthAfterReturn = await page.evaluate(() => window.history.length)
    expect(historyLengthAfterReturn).toBeLessThanOrEqual(historyLengthBeforeOpen + 1)

    // Viewing monitor (/instance/:id) and returning back to mobile editor + list still restores session context.
    await returnedButton.click()
    await page.getByRole('button', { name: '查看监控' }).click()
    await expect(page).toHaveURL('/instance/dd000021-0000-0000-0000-000000000000')
    await page.goBack()
    await expect(page).toHaveURL(/section=nodes&node=dd000021-/)
    await expect(page.getByRole('heading', { name: 'Scroll Node 12', exact: true })).toBeVisible()
    await page.getByRole('button', { name: '返回节点列表' }).click()
    await expect(page).toHaveURL('/admin?section=nodes')
    await expect(searchInput).toHaveValue('Scroll Node')
    await expect(providerFilter).toHaveAttribute('aria-pressed', 'true')
    await expect(list.getByRole('button', { name: /Scroll Node 12/ })).toBeFocused()
    await list.getByRole('button', { name: /Scroll Node 12/ }).click()
  }

  // Danger section has aligned >=44px controls and concrete confirmation verbs with target node name.
  const dangerSection = page.getByRole('region', { name: '危险操作' })
  const dangerButtons = dangerSection.getByRole('button')
  await expect(dangerButtons).toHaveCount(4)
  const boxes = await Promise.all([0, 1, 2, 3].map(index => dangerButtons.nth(index).boundingBox()))
  for (const box of boxes) {
    expect(box).not.toBeNull()
    expect(box!.height).toBeGreaterThanOrEqual(44)
  }
  expect(Math.abs(boxes[0]!.height - boxes[1]!.height)).toBeLessThanOrEqual(1)
  expect(Math.abs(boxes[2]!.height - boxes[3]!.height)).toBeLessThanOrEqual(1)

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

test('dirty default desktop edit survives viewport narrowing across 1024px and slow GET removal invalidates an open delete dialog', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  let nodes = buildSyntheticNodes(initialState.nodes[0])
  let holdNextGet = false
  let releaseHeldGet: (() => void) | null = null
  let deleteCalls = 0

  await page.route('**/api/admin/state', async (route) => {
    if (holdNextGet) {
      holdNextGet = false
      await new Promise<void>((resolve) => {
        releaseHeldGet = resolve
      })
      // Remove Alpha SG and reorder remaining nodes while the delete dialog for Alpha SG is open.
      nodes = nodes.filter(item => item.id !== 'aa112233-0000-0000-0000-000000000001')
    }
    await route.fulfill({
      json: {
        ...initialState,
        nodes,
      },
    })
  })
  await page.route('**/api/admin/nodes/*/delete', async (route) => {
    deleteCalls++
    await route.fulfill({ json: { ok: true } })
  })

  const originalViewport = page.viewportSize()
  await page.setViewportSize({ width: 1280, height: 800 })
  try {
    await page.goto('/admin?section=nodes')
    const remarkInput = page.getByLabel('公开备注')
    await expect(page.getByRole('heading', { name: 'Alpha SG', exact: true })).toBeVisible()
    await remarkInput.fill('Dirty before viewport resize')

    // Narrowing viewport across 1024px must keep Alpha SG selected and preserve dirty draft under guard.
    await page.setViewportSize({ width: 412, height: 915 })
    await expect(page.getByRole('heading', { name: 'Alpha SG', exact: true })).toBeVisible()
    await expect(remarkInput).toHaveValue('Dirty before viewport resize')
    const backButton = page.getByRole('button', { name: '返回节点列表' })
    await backButton.click()
    const discardDialog = page.getByRole('dialog', { name: '放弃未保存修改？' })
    await expect(discardDialog).toBeVisible()
    await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
    await expect(remarkInput).toHaveValue('Dirty before viewport resize')

    // Restore desktop viewport and discard draft so we can start a slow top refresh cleanly.
    await page.setViewportSize({ width: 1280, height: 800 })
    await page.getByRole('button', { name: '刷新', exact: true }).click()
    await discardDialog.getByRole('button', { name: '放弃修改并刷新' }).click()
    await expect(remarkInput).toHaveValue('Primary Singapore edge')

    // Start a slow top GET, open Delete dialog for Alpha SG, then let GET remove Alpha SG:
    // dialog must stay bound to Alpha SG, show stale invalidation alert, disable confirm, and never target Beta JP.
    holdNextGet = true
    await page.getByRole('button', { name: '刷新', exact: true }).click()
    await expect.poll(() => releaseHeldGet !== null).toBe(true)

    await page.getByRole('button', { name: '删除节点' }).click()
    const deleteDialog = page.getByRole('dialog', { name: '删除节点' })
    await expect(deleteDialog).toContainText('Alpha SG')

    releaseHeldGet?.()
    releaseHeldGet = null
    await expect(deleteDialog.getByRole('alert')).toContainText('节点「Alpha SG」已不存在')
    await expect(deleteDialog.getByRole('button', { name: '确认删除节点' })).toBeDisabled()
    expect(deleteCalls).toBe(0)
    await deleteDialog.getByRole('button', { name: '取消' }).click()
    await expect(deleteDialog).toBeHidden()
  }
  finally {
    if (originalViewport)
      await page.setViewportSize(originalViewport)
  }
})

test('explicit invalid node deep link shows not-found, revoked node disables rotate/revoke, and delete with refresh failure returns to list with resync', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  let nodes = buildSyntheticNodes(initialState.nodes[0])
  let failRefreshAfterDelete = false

  await page.route('**/api/admin/state', async (route) => {
    if (failRefreshAfterDelete) {
      await route.fulfill({
        status: 503,
        json: { error: 'Post-delete state refresh failed' },
      })
      return
    }
    await route.fulfill({
      json: {
        ...initialState,
        nodes,
      },
    })
  })
  await page.route('**/api/admin/nodes/*/delete', async (route) => {
    nodes = nodes.filter(item => item.id !== 'bb445566-0000-0000-0000-000000000002')
    failRefreshAfterDelete = true
    await route.fulfill({ json: { ok: true } })
  })

  // Revoked node disables rotate & revoke and explains re-enrollment creates a new independent node.
  await page.goto('/admin?section=nodes&node=cc778899-0000-0000-0000-000000000003')
  await expect(page.getByRole('heading', { name: 'Gamma US', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: '轮换 Agent 凭据' })).toBeDisabled()
  await expect(page.getByRole('button', { name: '撤销 Agent 凭据' })).toBeDisabled()
  await expect(page.getByText('独立新节点')).toBeVisible()

  // Confirmed delete with post-delete refresh failure closes dialog, returns cleanly to /admin?section=nodes (not "not found"), and exposes read-only resync.
  await page.goto('/admin?section=nodes&node=bb445566-0000-0000-0000-000000000002')
  await expect(page.getByRole('heading', { name: 'Beta JP', exact: true })).toBeVisible()
  await page.getByRole('button', { name: '删除节点' }).click()
  const deleteDialog = page.getByRole('dialog', { name: '删除节点' })
  await deleteDialog.getByRole('button', { name: '确认删除节点' }).click()
  await expect(deleteDialog).toBeHidden()
  await expect(page).toHaveURL('/admin?section=nodes')
  await expect(page.getByText('未找到该节点')).toHaveCount(0)
  await expect(page.getByRole('alert')).toContainText('Post-delete state refresh failed')
  failRefreshAfterDelete = false
  await page.getByRole('button', { name: '重新同步', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: '已重新同步最新状态' })).toBeVisible()
  await expect(page.getByRole('list', { name: '可管理节点' })).toBeVisible()

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
