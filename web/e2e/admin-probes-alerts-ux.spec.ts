import { expect, test } from '@playwright/test'
import { login, waitForNode } from './auth'

function buildSyntheticNodes(baseNode: Record<string, unknown>) {
  return [
    {
      ...baseNode,
      id: '11111111-0000-0000-0000-000000000001',
      name: 'Alpha SG',
      region: 'SG',
      group: 'Asia',
      hidden: false,
      disabled: false,
    },
    {
      ...baseNode,
      id: '22222222-0000-0000-0000-000000000002',
      name: 'Revoked JP',
      region: 'JP',
      group: 'Asia',
      hidden: false,
      disabled: true,
    },
  ]
}

test('probes management highlights active edits, protects drafts, preserves #312 scope checks, and reconciles uncertain saves and deletes', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  const nodes = buildSyntheticNodes(initialState.nodes[0])
  let probes = [
    {
      id: 'aaaaaaaa-0000-0000-0000-000000000001',
      name: 'Global Ping',
      weight: 0,
      kind: 'icmp',
      target: 'example.com',
      interval_seconds: 60,
      timeout_seconds: 5,
      enabled: true,
      node_ids: [],
    },
    {
      id: 'bbbbbbbb-0000-0000-0000-000000000002',
      name: 'Scoped TCP',
      weight: 1,
      kind: 'tcp',
      target: 'example.com:443',
      interval_seconds: 30,
      timeout_seconds: 5,
      enabled: true,
      node_ids: [
        '11111111-0000-0000-0000-000000000001',
        '22222222-0000-0000-0000-000000000002',
      ],
    },
  ]
  let saveReturn502 = false
  let deleteGlobal503 = true
  let failStateRefresh = false
  let scopedDeleteCalls = 0

  await page.route('**/api/v1/nodes/*/probes*', route => route.fulfill({
    json: {
      tasks: [],
      records: [],
      chart: [],
      summary: [],
      chart_source_points: 0,
      chart_bucket_ms: 60_000,
      range_start_unix_ms: 0,
      range_end_unix_ms: 0,
    },
  }))
  await page.route('**/api/admin/state', async (route) => {
    if (failStateRefresh) {
      await route.fulfill({
        status: 503,
        json: { error: 'Synthetic post-delete probe refresh failure' },
      })
      return
    }
    await route.fulfill({
      json: {
        ...initialState,
        nodes,
        probes,
      },
    })
  })
  await page.route('**/api/admin/probes', async (route) => {
    const body = route.request().postDataJSON()
    const idx = probes.findIndex(item => item.id === body.id)
    if (idx >= 0) {
      probes[idx] = {
        ...probes[idx],
        ...body,
      }
    }
    if (saveReturn502) {
      saveReturn502 = false
      await route.fulfill({
        status: 502,
        json: { error: 'Synthetic probe save 502' },
      })
      return
    }
    await route.fulfill({
      json: { ok: true },
    })
  })
  await page.route('**/api/admin/probes/*/delete', async (route) => {
    const url = route.request().url()
    if (url.includes('aaaaaaaa-0000-0000-0000-000000000001') && deleteGlobal503) {
      deleteGlobal503 = false
      // Simulate storage committing probe delete before metrics cleanup returns structured 503.
      probes = probes.filter(item => item.id !== 'aaaaaaaa-0000-0000-0000-000000000001')
      await route.fulfill({
        status: 503,
        json: { error: 'Probe metrics cleanup failed' },
      })
      return
    }
    if (url.includes('bbbbbbbb-0000-0000-0000-000000000002')) {
      scopedDeleteCalls++
      probes = probes.filter(item => item.id !== 'bbbbbbbb-0000-0000-0000-000000000002')
      failStateRefresh = true
    }
    await route.fulfill({
      json: { ok: true },
    })
  })

  // Prime same-document admin history (site -> probes) so browser Back stays in-app.
  await page.goto('/admin?section=site')
  const navigation = page.getByRole('navigation', { name: '管理页面' })
  const siteLink = navigation.getByRole('link', { name: '站点设置', exact: true })
  const probesLink = navigation.getByRole('link', { name: '探测', exact: true })
  await probesLink.click()
  await expect(page).toHaveURL('/admin?section=probes')

  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)
  expect(overflow).toBeLessThanOrEqual(0)

  const globalRow = page.getByRole('listitem').filter({ hasText: 'Global Ping' })
  const scopedRow = page.getByRole('listitem').filter({ hasText: 'Scoped TCP' })
  await expect(globalRow).toContainText('全部活动节点')
  await expect(scopedRow).toContainText('指定 2 个节点')
  await expect(scopedRow).toContainText('含 1 个已撤销节点')

  // Row buttons aligned >= 44px.
  const editBox = await scopedRow.getByRole('button', { name: '编辑' }).boundingBox()
  const deleteBox = await scopedRow.getByRole('button', { name: '删除' }).boundingBox()
  expect(editBox!.height).toBeGreaterThanOrEqual(44)
  expect(deleteBox!.height).toBeGreaterThanOrEqual(44)
  expect(Math.abs(editBox!.height - deleteBox!.height)).toBeLessThanOrEqual(1)

  // Editing highlights the row and moves focus to the probe form heading.
  await scopedRow.getByRole('button', { name: '编辑' }).click()
  await expect(scopedRow).toHaveAttribute('data-editing', 'true')
  await expect(scopedRow).toContainText('正在编辑')
  await expect(page.getByRole('heading', { name: '编辑探测任务' })).toBeFocused()

  // #312 protections: revoked node blocked inline; clearing all nodes requires explicit global-scope confirmation.
  await page.getByRole('button', { name: '保存任务' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '已撤销或不存在' })).toBeVisible()
  await page.getByLabel('Revoked JP（凭据已撤销）').uncheck()
  await page.getByLabel('Alpha SG', { exact: true }).uncheck()
  await expect(page.getByLabel('确认改为应用于所有活动节点')).toBeVisible()
  await page.getByRole('button', { name: '保存任务' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '静默扩大到所有活动节点' })).toBeVisible()
  await page.getByLabel('Alpha SG', { exact: true }).check()

  // Dirty probe draft guards switching tasks, switching sections, browser Back, and top Refresh.
  const nameInput = page.getByLabel('名称', { exact: true })
  await nameInput.fill('Scoped TCP Dirty')
  const globalEditButton = globalRow.getByRole('button', { name: '编辑' })
  await globalEditButton.click()
  const discardDialog = page.getByRole('dialog', { name: '放弃未保存修改？' })
  await expect(discardDialog).toBeVisible()
  await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(globalEditButton).toBeFocused()
  await expect(nameInput).toHaveValue('Scoped TCP Dirty')

  await siteLink.click()
  await expect(discardDialog).toBeVisible()
  await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(page).toHaveURL('/admin?section=probes')

  await page.goBack()
  await expect(discardDialog).toBeVisible()
  await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(page).toHaveURL('/admin?section=probes')

  await page.getByRole('button', { name: '刷新', exact: true }).click()
  await expect(discardDialog).toBeVisible()
  await discardDialog.getByRole('button', { name: '放弃修改并刷新' }).click()
  await expect(nameInput).toHaveValue('Scoped TCP')

  // Save 502 -> user types newer edit -> resync confirms prior save while preserving newer edit.
  await page.getByLabel('Revoked JP（凭据已撤销）').uncheck()
  await nameInput.fill('Scoped TCP Applied')
  saveReturn502 = true
  await page.getByRole('button', { name: '保存任务' }).click()
  await expect(page.getByRole('alert').filter({ hasText: 'Synthetic probe save 502' })).toBeVisible()
  await nameInput.fill('Scoped TCP Newer Edit')
  const saveButton = page.getByRole('button', { name: '保存任务' })
  const resyncButton = page.getByRole('button', { name: '重新同步', exact: true })
  const saveBox = await saveButton.boundingBox()
  const resyncBox = await resyncButton.boundingBox()
  expect(saveBox!.height).toBeGreaterThanOrEqual(44)
  expect(resyncBox!.height).toBeGreaterThanOrEqual(44)
  expect(Math.abs(saveBox!.height - resyncBox!.height)).toBeLessThanOrEqual(1)
  await resyncButton.click()
  await expect(page.getByRole('status').filter({ hasText: '您之后的新修改已保留' })).toBeVisible()
  await expect(nameInput).toHaveValue('Scoped TCP Newer Edit')

  // Discard newer edit via cancel button so we can test delete safeguards cleanly.
  await page.getByRole('button', { name: '取消编辑' }).click()
  await discardDialog.getByRole('button', { name: '放弃修改并切换' }).click()
  await expect(page.getByRole('heading', { name: '新增探测任务' })).toBeVisible()

  // Structured 503 on probe delete locks per target across section remounts and reconciles via read-only resync.
  await globalRow.getByRole('button', { name: '删除' }).click()
  const deleteDialog = page.getByRole('dialog', { name: '删除探测任务' })
  const confirmDeleteButton = deleteDialog.getByRole('button', { name: '确认删除探测任务' })
  await confirmDeleteButton.click()
  await expect(deleteDialog.getByRole('alert')).toContainText('Probe metrics cleanup failed')
  await expect(confirmDeleteButton).toBeDisabled()
  await deleteDialog.getByRole('button', { name: '取消' }).click()

  await siteLink.click()
  await expect(page).toHaveURL('/admin?section=site')
  await probesLink.click()
  await expect(page).toHaveURL('/admin?section=probes')
  await expect(page.getByRole('alert').filter({ hasText: '删除探测任务「Global Ping」结果未确认' })).toBeVisible()
  await page.getByRole('button', { name: '重新同步', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: '已核实：探测任务「Global Ping」已删除' })).toBeVisible()
  await expect(globalRow).toHaveCount(0)

  // Confirmed delete with post-delete GET 503 hides deleted probe immediately and exposes read-only resync without replaying DELETE.
  const updatedScopedRow = page.getByRole('listitem').filter({ hasText: 'Scoped TCP Applied' })
  await updatedScopedRow.getByRole('button', { name: '删除' }).click()
  await deleteDialog.getByRole('button', { name: '确认删除探测任务' }).click()
  await expect(deleteDialog).toBeHidden()
  await expect(updatedScopedRow).toHaveCount(0)
  await expect(page.getByRole('alert').filter({ hasText: 'Synthetic post-delete probe refresh failure' })).toBeVisible()
  failStateRefresh = false
  await page.getByRole('button', { name: '重新同步', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: '已重新同步最新状态' })).toBeVisible()
  expect(scopedDeleteCalls).toBe(1)
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})

test('alerts management isolates channel and rule drafts, shows scope and channel summaries, preserves #312 checks, and bypasses guards on 401', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  const nodes = buildSyntheticNodes(initialState.nodes[0])
  const channels = [
    {
      id: 'cccccccc-0000-0000-0000-000000000001',
      name: 'Primary Hook',
      kind: 'webhook',
      url: 'https://example.invalid/synthetic-hook-1',
      enabled: true,
    },
    {
      id: 'cccccccc-0000-0000-0000-000000000002',
      name: 'Backup Hook',
      kind: 'webhook',
      url: 'https://example.invalid/synthetic-hook-2',
      enabled: false,
    },
  ]
  const rules = [
    {
      id: 'dddddddd-0000-0000-0000-000000000001',
      name: 'In-App Global Rule',
      metric: 'offline',
      threshold: 0,
      duration_seconds: 120,
      cooldown_seconds: 900,
      node_ids: [],
      channel_ids: [],
      enabled: true,
    },
    {
      id: 'dddddddd-0000-0000-0000-000000000002',
      name: 'Scoped Stale Rule',
      metric: 'cpu',
      threshold: 85,
      duration_seconds: 120,
      cooldown_seconds: 900,
      node_ids: [
        '11111111-0000-0000-0000-000000000001',
        '22222222-0000-0000-0000-000000000002',
      ],
      channel_ids: [
        'cccccccc-0000-0000-0000-000000000001',
        'eeeeeeee-0000-0000-0000-000000000999',
      ],
      enabled: true,
    },
  ]
  let expireRuleSaveWith401 = false
  let expired = false

  await page.route('**/api/admin/state', route => route.fulfill({
    json: {
      ...initialState,
      nodes,
      channels,
      alert_rules: rules,
    },
  }))
  await page.route('**/api/admin/channels', async (route) => {
    const body = route.request().postDataJSON()
    const idx = channels.findIndex(item => item.id === body.id)
    if (idx >= 0) {
      channels[idx] = {
        ...channels[idx],
        ...body,
      }
    }
    await route.fulfill({
      json: { ok: true },
    })
  })
  await page.route('**/api/admin/alert-rules', async (route) => {
    if (expireRuleSaveWith401) {
      expired = true
      await route.fulfill({
        status: 401,
        json: { error: 'Session expired' },
      })
      return
    }
    const body = route.request().postDataJSON()
    const idx = rules.findIndex(item => item.id === body.id)
    if (idx >= 0) {
      rules[idx] = {
        ...rules[idx],
        ...body,
      }
    }
    await route.fulfill({
      json: { ok: true },
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

  await page.goto('/admin?section=alerts')
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)
  expect(overflow).toBeLessThanOrEqual(0)

  const primaryChannelRow = page.getByRole('listitem').filter({ hasText: 'Primary Hook' })
  const backupChannelRow = page.getByRole('listitem').filter({ hasText: 'Backup Hook' })
  const globalRuleRow = page.getByRole('listitem').filter({ hasText: 'In-App Global Rule' })
  const scopedRuleRow = page.getByRole('listitem').filter({ hasText: 'Scoped Stale Rule' })

  // List summaries show scope, revoked nodes, channel info, in-app-only label, and never expose webhook URLs in rows.
  await expect(primaryChannelRow).not.toContainText('synthetic-hook-1')
  await expect(globalRuleRow).toContainText('全部活动节点')
  await expect(globalRuleRow).toContainText('仅站内记录')
  await expect(scopedRuleRow).toContainText('指定 2 个节点')
  await expect(scopedRuleRow).toContainText('含 1 个已撤销节点')
  await expect(scopedRuleRow).toContainText('通知渠道：Primary Hook 等 2 个')
  await expect(scopedRuleRow).toContainText('含 1 个已失效渠道')

  // 1. Edit Primary Hook and make channel draft dirty.
  await primaryChannelRow.getByRole('button', { name: '编辑' }).click()
  await expect(primaryChannelRow).toHaveAttribute('data-editing', 'true')
  await expect(page.getByRole('heading', { name: '编辑渠道' })).toBeFocused()
  const channelNameInput = page.getByLabel('渠道名称', { exact: true })
  await channelNameInput.fill('Primary Hook Dirty')

  // 2. Switching to edit an alert rule while channel is dirty MUST NOT trigger a discard dialog or drop channel draft.
  await scopedRuleRow.getByRole('button', { name: '编辑' }).click()
  const discardDialog = page.getByRole('dialog', { name: '放弃未保存修改？' })
  await expect(discardDialog).toHaveCount(0)
  await expect(scopedRuleRow).toHaveAttribute('data-editing', 'true')
  await expect(page.getByRole('heading', { name: '编辑规则' })).toBeFocused()
  await expect(channelNameInput).toHaveValue('Primary Hook Dirty')

  // 3. Make rule draft dirty too and verify #312 revoked node + stale channel + global scope safeguards.
  const ruleNameInput = page.getByLabel('规则名称', { exact: true })
  await ruleNameInput.fill('Scoped Rule Dirty')
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '已撤销或不存在' })).toBeVisible()
  await page.getByLabel('Revoked JP（凭据已撤销）').uncheck()
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '已失效的通知渠道' })).toBeVisible()
  await page.getByRole('button', { name: '移除失效渠道' }).click()

  // 4. Switching channel while BOTH drafts are dirty prompts ONLY for channel and preserves the dirty rule draft intact.
  await backupChannelRow.getByRole('button', { name: '编辑' }).click()
  await expect(discardDialog).toBeVisible()
  await expect(discardDialog).toContainText('不会影响下方告警规则草稿')
  await discardDialog.getByRole('button', { name: '放弃修改并切换' }).click()
  await expect(backupChannelRow).toHaveAttribute('data-editing', 'true')
  await expect(channelNameInput).toHaveValue('Backup Hook')
  await expect(ruleNameInput).toHaveValue('Scoped Rule Dirty')

  // 5. Make Backup Hook dirty too; switching rule prompts ONLY for rule (cancel preserves both), and switching sections guards both drafts.
  await channelNameInput.fill('Backup Hook Saved Later')
  const globalRuleEditButton = globalRuleRow.getByRole('button', { name: '编辑' })
  await globalRuleEditButton.click()
  await expect(discardDialog).toBeVisible()
  await expect(discardDialog).toContainText('不会影响上方通知渠道草稿')
  await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(globalRuleEditButton).toBeFocused()
  await expect(channelNameInput).toHaveValue('Backup Hook Saved Later')
  await expect(ruleNameInput).toHaveValue('Scoped Rule Dirty')

  await page.getByRole('navigation', { name: '管理页面' }).getByRole('link', { name: '站点设置', exact: true }).click()
  await expect(discardDialog).toBeVisible()
  await expect(discardDialog).toContainText('通知渠道与告警规则')
  await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(page).toHaveURL('/admin?section=alerts')

  // 6. Save the RULE: rule saves & resets baseline cleanly while dirty channel draft stays intact!
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page.getByRole('status').filter({ hasText: '已保存告警规则' })).toBeVisible()
  await expect(ruleNameInput).toHaveValue('')
  await expect(channelNameInput).toHaveValue('Backup Hook Saved Later')

  // 7. Save the Channel: channel saves & resets cleanly.
  await page.getByRole('button', { name: '保存渠道' }).click()
  await expect(page.getByRole('status').filter({ hasText: '已保存通知渠道' })).toBeVisible()
  await expect(page.getByRole('listitem').filter({ hasText: 'Backup Hook Saved Later' })).toBeVisible()

  // 8. 401 session expiry while saving a dirty rule exits cleanly to /login without a discard prompt.
  await globalRuleRow.getByRole('button', { name: '编辑' }).click()
  await ruleNameInput.fill('Dirty before 401')
  expireRuleSaveWith401 = true
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page).toHaveURL(/\/login\?redirect=/)
  await expect(discardDialog).toBeHidden()
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})
