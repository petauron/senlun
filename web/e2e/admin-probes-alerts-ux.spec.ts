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

const EMPTY_PROBE_HISTORY = {
  tasks: [],
  records: [],
  chart: [],
  summary: [],
  chart_source_points: 0,
  chart_bucket_ms: 60_000,
  range_start_unix_ms: 0,
  range_end_unix_ms: 0,
}

test('probes management highlights active edits, syncs dynamic #312 scope after 502, locks confirmed top refresh, and preserves dirty drafts on read-only delete resync', async ({ page }) => {
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
      node_ids: [] as string[],
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
  let holdNextStateGet = false
  let failHeldStateGet = false
  let releaseHeldStateGet: (() => void) | null = null
  let deleteScoped503 = true
  let failPostDeleteRefresh = false
  let globalDeleteCalls = 0

  await page.route('**/api/v1/nodes/*/probes*', route => route.fulfill({
    json: EMPTY_PROBE_HISTORY,
  }))
  await page.route('**/api/admin/state', async (route) => {
    if (holdNextStateGet) {
      holdNextStateGet = false
      await new Promise<void>((resolve) => {
        releaseHeldStateGet = resolve
      })
      if (failHeldStateGet) {
        failHeldStateGet = false
        await route.fulfill({
          status: 503,
          json: { error: 'Synthetic top refresh failure' },
        })
        return
      }
    }
    if (failPostDeleteRefresh) {
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
    else {
      probes.push(body)
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
    if (url.includes('bbbbbbbb-0000-0000-0000-000000000002') && deleteScoped503) {
      deleteScoped503 = false
      // Simulate backend committing probe delete before metrics cleanup returns structured 503.
      probes = probes.filter(item => item.id !== 'bbbbbbbb-0000-0000-0000-000000000002')
      await route.fulfill({
        status: 503,
        json: { error: 'Probe metrics cleanup failed' },
      })
      return
    }
    if (url.includes('aaaaaaaa-0000-0000-0000-000000000001')) {
      globalDeleteCalls++
      probes = probes.filter(item => item.id !== 'aaaaaaaa-0000-0000-0000-000000000001')
      failPostDeleteRefresh = true
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

  const probeList = page.getByRole('list', { name: '探测任务列表' })
  const globalRow = probeList.getByRole('listitem').filter({ hasText: 'Global Ping' })
  const scopedRow = probeList.getByRole('listitem').filter({ hasText: 'Scoped TCP' })
  await expect(globalRow).toContainText('全部活动节点')
  await expect(scopedRow).toContainText('指定 2 个节点')
  await expect(scopedRow).toContainText('含 1 个已撤销节点')

  const editBox = await scopedRow.getByRole('button', { name: '编辑' }).boundingBox()
  const deleteBox = await scopedRow.getByRole('button', { name: '删除' }).boundingBox()
  expect(editBox!.height).toBeGreaterThanOrEqual(44)
  expect(deleteBox!.height).toBeGreaterThanOrEqual(44)
  expect(Math.abs(editBox!.height - deleteBox!.height)).toBeLessThanOrEqual(1)

  // Editing highlights row & focuses heading; #312 blocks revoked node and silent widening.
  await scopedRow.getByRole('button', { name: '编辑' }).click()
  await expect(scopedRow).toHaveAttribute('data-editing', 'true')
  await expect(scopedRow).toContainText('正在编辑')
  await expect(page.getByRole('heading', { name: '编辑探测任务' })).toBeFocused()

  await page.getByRole('button', { name: '保存任务' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '已撤销或不存在' })).toBeVisible()
  await page.getByLabel('Revoked JP（凭据已撤销）').uncheck()
  await page.getByLabel('Alpha SG', { exact: true }).uncheck()
  await expect(page.getByLabel('确认改为应用于所有活动节点')).toBeVisible()
  await page.getByRole('button', { name: '保存任务' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '静默扩大到所有活动节点' })).toBeVisible()

  // Dirty guard: cancel restores trigger focus; section & browser Back guarded; confirm focuses form heading!
  const nameInput = page.getByLabel('名称', { exact: true })
  await nameInput.fill('Scoped TCP Dirty')
  const globalEditButton = globalRow.getByRole('button', { name: '编辑' })
  await globalEditButton.click()
  const discardDialog = page.getByRole('dialog', { name: '放弃未保存修改？' })
  await expect(discardDialog).toBeVisible()
  await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(globalEditButton).toBeFocused()

  await siteLink.click()
  await expect(discardDialog).toBeVisible()
  await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(page).toHaveURL('/admin?section=probes')

  await page.goBack()
  await expect(discardDialog).toBeVisible()
  await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(page).toHaveURL('/admin?section=probes')

  await globalEditButton.click()
  await discardDialog.getByRole('button', { name: '放弃修改并切换' }).click()
  await expect(globalRow).toHaveAttribute('data-editing', 'true')
  await expect(page.getByRole('heading', { name: '编辑探测任务' })).toBeFocused()

  // Dynamic #312 scope sync: Global Ping starts global (guardGlobalScope=false), user narrows to Alpha SG -> 502 (server saved [Alpha]),
  // user unchecks Alpha SG & edits name before resync -> resync preserves newer edit AND dynamically enables #312 guard blocking silent widening!
  await page.getByLabel('Alpha SG', { exact: true }).check()
  saveReturn502 = true
  await page.getByRole('button', { name: '保存任务' }).click()
  await expect(page.getByRole('alert').filter({ hasText: 'Synthetic probe save 502' })).toBeVisible()
  await page.getByLabel('Alpha SG', { exact: true }).uncheck()
  await nameInput.fill('Global Ping Newer Edit')
  await page.getByRole('button', { name: '重新同步', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: '您之后的新修改已保留' })).toBeVisible()
  await expect(nameInput).toHaveValue('Global Ping Newer Edit')
  await expect(page.getByLabel('确认改为应用于所有活动节点')).toBeVisible()
  await page.getByRole('button', { name: '保存任务' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '静默扩大到所有活动节点' })).toBeVisible()

  // Confirmed top refresh locks inputs while pending; failed refresh preserves dirty draft; succeeded refresh resets cleanly.
  await page.getByLabel('确认改为应用于所有活动节点').check()
  holdNextStateGet = true
  failHeldStateGet = true
  await page.getByRole('button', { name: '刷新', exact: true }).click()
  await discardDialog.getByRole('button', { name: '放弃修改并刷新' }).click()
  await expect.poll(() => releaseHeldStateGet !== null).toBe(true)
  await expect(nameInput).toBeDisabled()
  await expect(page.getByRole('button', { name: '保存任务' })).toBeDisabled()
  releaseHeldStateGet?.()
  releaseHeldStateGet = null
  await expect(page.getByRole('alert').filter({ hasText: 'Synthetic top refresh failure' })).toBeVisible()
  await expect(nameInput).toBeEnabled()
  await expect(nameInput).toHaveValue('Global Ping Newer Edit')

  await page.getByRole('button', { name: '刷新', exact: true }).click()
  await discardDialog.getByRole('button', { name: '放弃修改并刷新' }).click()
  await expect(nameInput).toHaveValue('Global Ping')

  // Structured 503 on probe delete -> user edits deleted target dirty -> read-only resync verifies removal WITHOUT wiping dirty draft!
  await scopedRow.getByRole('button', { name: '删除' }).click()
  const deleteDialog = page.getByRole('dialog', { name: '删除探测任务' })
  const confirmDeleteButton = deleteDialog.getByRole('button', { name: '确认删除探测任务' })
  await confirmDeleteButton.click()
  await expect(deleteDialog.getByRole('alert')).toContainText('Probe metrics cleanup failed')
  await expect(confirmDeleteButton).toBeDisabled()
  await deleteDialog.getByRole('button', { name: '取消' }).click()

  await scopedRow.getByRole('button', { name: '编辑' }).click()
  await nameInput.fill('Dirty edit kept across read-only delete resync')
  await page.getByRole('button', { name: '重新同步', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: '已核实：探测任务「Scoped TCP」已删除' })).toBeVisible()
  await expect(scopedRow).toHaveCount(0)
  await expect(nameInput).toHaveValue('Dirty edit kept across read-only delete resync')
  await expect(page.getByRole('alert').filter({ hasText: '在最新同步中已不存在' })).toBeVisible()
  await expect(page.getByRole('button', { name: '保存任务' })).toBeDisabled()

  await page.getByRole('button', { name: '取消编辑' }).click()
  await discardDialog.getByRole('button', { name: '放弃修改并切换' }).click()
  await expect(page.getByRole('heading', { name: '新增探测任务' })).toBeVisible()

  // Confirmed delete with post-delete GET 503 hides deleted probe immediately and resyncs read-only without replaying DELETE.
  await globalRow.getByRole('button', { name: '删除' }).click()
  await deleteDialog.getByRole('button', { name: '确认删除探测任务' }).click()
  await expect(deleteDialog).toBeHidden()
  await expect(globalRow).toHaveCount(0)
  await expect(page.getByRole('alert').filter({ hasText: 'Synthetic post-delete probe refresh failure' })).toBeVisible()
  failPostDeleteRefresh = false
  await page.getByRole('button', { name: '重新同步', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: '已重新同步最新状态' })).toBeVisible()
  expect(globalDeleteCalls).toBe(1)
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})

test('new probe, channel, and rule drafts reuse stable client UUIDs across 502 retries, guard unconfirmed new scopes, and avoid duplicate-name channel false positives', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  const nodes = buildSyntheticNodes(initialState.nodes[0])
  const probes: Array<Record<string, unknown>> = []
  const channels: Array<Record<string, unknown>> = []
  const rules: Array<Record<string, unknown>> = []
  const submittedProbeIds: string[] = []
  const submittedChannelIds: string[] = []
  const submittedRuleIds: string[] = []
  let failFirstProbeSave = true
  let failFirstChannelSave = true
  let failFirstRuleSave = true

  await page.route('**/api/v1/nodes/*/probes*', route => route.fulfill({
    json: EMPTY_PROBE_HISTORY,
  }))
  await page.route('**/api/admin/state', route => route.fulfill({
    json: {
      ...initialState,
      nodes,
      probes,
      channels,
      alert_rules: rules,
    },
  }))
  await page.route('**/api/admin/probes', async (route) => {
    const body = route.request().postDataJSON()
    submittedProbeIds.push(body.id)
    const idx = probes.findIndex(item => item.id === body.id)
    if (idx >= 0)
      probes[idx] = body
    else
      probes.push(body)
    if (failFirstProbeSave) {
      failFirstProbeSave = false
      await route.fulfill({
        status: 502,
        json: { error: 'Probe created before 502' },
      })
      return
    }
    await route.fulfill({
      json: { ok: true },
    })
  })
  await page.route('**/api/admin/channels', async (route) => {
    const body = route.request().postDataJSON()
    submittedChannelIds.push(body.id)
    if (failFirstChannelSave) {
      failFirstChannelSave = false
      // Simulate another admin creating a DIFFERENT channel with the SAME name & enabled flag, but a different URL/ID.
      channels.push({
        id: '99999999-0000-0000-0000-000000000099',
        name: body.name,
        kind: 'webhook',
        url: 'https://example.invalid/unrelated-same-name-hook',
        enabled: body.enabled,
      })
      await route.fulfill({
        status: 502,
        json: { error: 'Channel create dropped with 502' },
      })
      return
    }
    const idx = channels.findIndex(item => item.id === body.id)
    if (idx >= 0)
      channels[idx] = body
    else
      channels.push(body)
    await route.fulfill({
      json: { ok: true },
    })
  })
  await page.route('**/api/admin/alert-rules', async (route) => {
    const body = route.request().postDataJSON()
    submittedRuleIds.push(body.id)
    const idx = rules.findIndex(item => item.id === body.id)
    if (idx >= 0)
      rules[idx] = body
    else
      rules.push(body)
    if (failFirstRuleSave) {
      failFirstRuleSave = false
      await route.fulfill({
        status: 502,
        json: { error: 'Rule created before 502' },
      })
      return
    }
    await route.fulfill({
      json: { ok: true },
    })
  })

  // 1. New probe targeting Alpha SG -> 502 (persisted on server) -> unchecking Alpha SG before resync MUST NOT send global POST -> re-checking & retrying upserts SAME UUID without duplicate!
  await page.goto('/admin?section=probes')
  await page.getByLabel('名称', { exact: true }).fill('Idempotent Probe')
  await page.getByLabel('目标', { exact: true }).fill('example.com')
  await page.getByLabel('Alpha SG', { exact: true }).check()
  await page.getByRole('button', { name: '保存任务' }).click()
  await expect(page.getByRole('alert').filter({ hasText: 'Probe created before 502' })).toBeVisible()
  expect(submittedProbeIds).toHaveLength(1)

  await page.getByLabel('Alpha SG', { exact: true }).uncheck()
  await page.getByRole('button', { name: '保存任务' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '静默扩大到所有活动节点' })).toBeVisible()
  expect(submittedProbeIds).toHaveLength(1)

  await page.getByLabel('Alpha SG', { exact: true }).check()
  await page.getByRole('button', { name: '保存任务' }).click()
  await expect(page.getByRole('status').filter({ hasText: '已保存探测任务' })).toBeVisible()
  expect(submittedProbeIds).toHaveLength(2)
  expect(submittedProbeIds[0]).toBe(submittedProbeIds[1])
  await expect(page.getByRole('list', { name: '探测任务列表' }).getByRole('listitem').filter({ hasText: 'Idempotent Probe' })).toHaveCount(1)

  // 2. New channel duplicate-name / different-URL MUST NOT false-positive on resync or wipe URL; retry uses same client UUID.
  await page.goto('/admin?section=alerts')
  const channelNameInput = page.getByLabel('渠道名称', { exact: true })
  const webhookUrlInput = page.getByLabel('Webhook URL', { exact: true })
  await channelNameInput.fill('Duplicate Name Hook')
  await webhookUrlInput.fill('https://example.invalid/my-intended-hook')
  await page.getByRole('button', { name: '保存渠道' }).click()
  await expect(page.getByRole('alert').filter({ hasText: 'Channel create dropped with 502' })).toBeVisible()

  await page.getByRole('button', { name: '重新同步', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: '列表中尚未出现本次提交的新通知渠道' })).toBeVisible()
  await expect(webhookUrlInput).toHaveValue('https://example.invalid/my-intended-hook')

  await page.getByRole('button', { name: '保存渠道' }).click()
  await expect(page.getByRole('status').filter({ hasText: '已保存通知渠道' })).toBeVisible()
  expect(submittedChannelIds).toHaveLength(2)
  expect(submittedChannelIds[0]).toBe(submittedChannelIds[1])
  expect(channels.some(item => item.id === submittedChannelIds[0] && item.url === 'https://example.invalid/my-intended-hook')).toBe(true)

  // 3. New alert rule targeting Alpha SG -> 502 (persisted) -> unchecking Alpha SG before resync MUST NOT send global POST -> retry upserts SAME UUID!
  await page.getByLabel('规则名称', { exact: true }).fill('Idempotent Rule')
  await page.getByLabel('Alpha SG', { exact: true }).check()
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page.getByRole('alert').filter({ hasText: 'Rule created before 502' })).toBeVisible()
  expect(submittedRuleIds).toHaveLength(1)

  await page.getByLabel('Alpha SG', { exact: true }).uncheck()
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '静默扩大到所有活动节点' })).toBeVisible()
  expect(submittedRuleIds).toHaveLength(1)

  await page.getByLabel('Alpha SG', { exact: true }).check()
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page.getByRole('status').filter({ hasText: '已保存告警规则' })).toBeVisible()
  expect(submittedRuleIds).toHaveLength(2)
  expect(submittedRuleIds[0]).toBe(submittedRuleIds[1])
  await expect(page.getByRole('list', { name: '告警规则列表' }).getByRole('listitem').filter({ hasText: 'Idempotent Rule' })).toHaveCount(1)
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})

test('alerts management isolates channel and rule drafts, shows in-app-only zero-delivery summaries, preserves dirty drafts on delete resync, and bypasses guards on 401', async ({ page }) => {
  await login(page, '/admin')
  await waitForNode(page.request)
  const initialState = await (await page.request.get('/api/admin/state')).json()
  const nodes = buildSyntheticNodes(initialState.nodes[0])
  let channels = [
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
      node_ids: [] as string[],
      channel_ids: [] as string[],
      enabled: true,
    },
    {
      id: 'dddddddd-0000-0000-0000-000000000002',
      name: 'Disabled Only Rule',
      metric: 'memory',
      threshold: 90,
      duration_seconds: 120,
      cooldown_seconds: 900,
      node_ids: [] as string[],
      channel_ids: ['cccccccc-0000-0000-0000-000000000002'],
      enabled: true,
    },
    {
      id: 'dddddddd-0000-0000-0000-000000000003',
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
  let ruleSave502 = false
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
  await page.route('**/api/admin/channels/*/delete', async (route) => {
    channels = channels.filter(item => item.id !== 'cccccccc-0000-0000-0000-000000000002')
    await route.fulfill({
      status: 502,
      json: { error: 'Channel delete 502 after commit' },
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
    if (ruleSave502) {
      ruleSave502 = false
      await route.fulfill({
        status: 502,
        json: { error: 'Rule save 502 after narrowing scope' },
      })
      return
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

  const channelList = page.getByRole('list', { name: '通知渠道列表' })
  const ruleList = page.getByRole('list', { name: '告警规则列表' })
  const primaryChannelRow = channelList.getByRole('listitem').filter({ hasText: 'Primary Hook' })
  const backupChannelRow = channelList.getByRole('listitem').filter({ hasText: 'Backup Hook' })
  const globalRuleRow = ruleList.getByRole('listitem').filter({ hasText: 'In-App Global Rule' })
  const disabledOnlyRuleRow = ruleList.getByRole('listitem').filter({ hasText: 'Disabled Only Rule' })
  const scopedRuleRow = ruleList.getByRole('listitem').filter({ hasText: 'Scoped Stale Rule' })

  // List summaries show scope, revoked nodes, channel info, zero-delivery in-app-only wording, and never expose webhook URLs in rows.
  await expect(primaryChannelRow).not.toContainText('synthetic-hook-1')
  await expect(globalRuleRow).toContainText('全部活动节点')
  await expect(globalRuleRow).toContainText('仅站内记录')
  await expect(disabledOnlyRuleRow).toContainText('仅站内记录（无启用外发渠道）')
  await expect(disabledOnlyRuleRow).toContainText('含 1 个已停用渠道')
  await expect(scopedRuleRow).toContainText('指定 2 个节点')
  await expect(scopedRuleRow).toContainText('含 1 个已撤销节点')
  await expect(scopedRuleRow).toContainText('通知渠道：Primary Hook 等 2 个')
  await expect(scopedRuleRow).toContainText('含 1 个已失效渠道')

  // 1. Edit Primary Hook dirty, then edit Scoped Stale Rule: zero discard dialog and channel draft stays intact.
  await primaryChannelRow.getByRole('button', { name: '编辑' }).click()
  await expect(primaryChannelRow).toHaveAttribute('data-editing', 'true')
  await expect(page.getByRole('heading', { name: '编辑渠道' })).toBeFocused()
  const channelNameInput = page.getByLabel('渠道名称', { exact: true })
  await channelNameInput.fill('Primary Hook Dirty')

  await scopedRuleRow.getByRole('button', { name: '编辑' }).click()
  const discardDialog = page.getByRole('dialog', { name: '放弃未保存修改？' })
  await expect(discardDialog).toHaveCount(0)
  await expect(scopedRuleRow).toHaveAttribute('data-editing', 'true')
  await expect(page.getByRole('heading', { name: '编辑规则' })).toBeFocused()
  await expect(channelNameInput).toHaveValue('Primary Hook Dirty')

  // 2. Make rule dirty too, verify #312 revoked node + stale channel pruning.
  const ruleNameInput = page.getByLabel('规则名称', { exact: true })
  await ruleNameInput.fill('Scoped Rule Dirty')
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '已撤销或不存在' })).toBeVisible()
  await page.getByLabel('Revoked JP（凭据已撤销）').uncheck()
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '已失效的通知渠道' })).toBeVisible()
  await page.getByRole('button', { name: '移除失效渠道' }).click()

  // 3. Confirmed channel switch while BOTH dirty: only resets channel, focuses 编辑渠道 heading, preserves dirty rule!
  await backupChannelRow.getByRole('button', { name: '编辑' }).click()
  await expect(discardDialog).toBeVisible()
  await expect(discardDialog).toContainText('不会影响下方告警规则草稿')
  await discardDialog.getByRole('button', { name: '放弃修改并切换' }).click()
  await expect(backupChannelRow).toHaveAttribute('data-editing', 'true')
  await expect(page.getByRole('heading', { name: '编辑渠道' })).toBeFocused()
  await expect(channelNameInput).toHaveValue('Backup Hook')
  await expect(ruleNameInput).toHaveValue('Scoped Rule Dirty')

  // 4. Dirty Backup Hook too: rule switch cancel restores focus, section switch names both drafts.
  await channelNameInput.fill('Backup Hook Saved Later')
  const globalRuleEditButton = globalRuleRow.getByRole('button', { name: '编辑' })
  await globalRuleEditButton.click()
  await expect(discardDialog).toBeVisible()
  await expect(discardDialog).toContainText('不会影响上方通知渠道草稿')
  await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(globalRuleEditButton).toBeFocused()

  await page.getByRole('navigation', { name: '管理页面' }).getByRole('link', { name: '站点设置', exact: true }).click()
  await expect(discardDialog).toBeVisible()
  await expect(discardDialog).toContainText('通知渠道与告警规则')
  await discardDialog.getByRole('button', { name: '取消，继续编辑' }).click()
  await expect(page).toHaveURL('/admin?section=alerts')

  // 5. Save RULE: resets ONLY rule baseline while preserving dirty Backup Hook channel draft.
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page.getByRole('status').filter({ hasText: '已保存告警规则' })).toBeVisible()
  await expect(ruleNameInput).toHaveValue('')
  await expect(channelNameInput).toHaveValue('Backup Hook Saved Later')

  // 6. Save CHANNEL: resets ONLY channel baseline.
  await page.getByRole('button', { name: '保存渠道' }).click()
  await expect(page.getByRole('status').filter({ hasText: '已保存通知渠道' })).toBeVisible()
  const updatedBackupChannelRow = channelList.getByRole('listitem').filter({ hasText: 'Backup Hook Saved Later' })
  await expect(updatedBackupChannelRow).toBeVisible()

  // 7. Dynamic #312 rule scope sync after 502: In-App Global Rule narrowed to Alpha SG -> 502 (applied) -> uncheck Alpha SG & edit name -> resync preserves edit AND blocks silent widening!
  await globalRuleRow.getByRole('button', { name: '编辑' }).click()
  await page.getByLabel('Alpha SG', { exact: true }).check()
  ruleSave502 = true
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page.getByRole('alert').filter({ hasText: 'Rule save 502 after narrowing scope' })).toBeVisible()
  await page.getByLabel('Alpha SG', { exact: true }).uncheck()
  await ruleNameInput.fill('In-App Global Rule Newer')
  await page.getByRole('button', { name: '重新同步', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: '您之后的新修改已保留' })).toBeVisible()
  await expect(page.getByLabel('确认改为应用于所有活动节点')).toBeVisible()
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page.getByRole('alert').filter({ hasText: '静默扩大到所有活动节点' })).toBeVisible()

  // 8. Uncertain channel delete 502 -> edit channel dirty -> read-only resync verifies deletion WITHOUT wiping dirty channel draft (and leaves dirty rule untouched)!
  await updatedBackupChannelRow.getByRole('button', { name: '删除' }).click()
  const deleteDialog = page.getByRole('dialog', { name: '删除通知渠道' })
  await deleteDialog.getByRole('button', { name: '确认删除通知渠道' }).click()
  await expect(deleteDialog.getByRole('alert')).toContainText('Channel delete 502 after commit')
  await deleteDialog.getByRole('button', { name: '取消' }).click()

  await updatedBackupChannelRow.getByRole('button', { name: '编辑' }).click()
  await channelNameInput.fill('Dirty channel kept after delete resync')
  await page.getByRole('button', { name: '重新同步', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: '已核实：通知渠道「Backup Hook Saved Later」已删除' })).toBeVisible()
  await expect(channelNameInput).toHaveValue('Dirty channel kept after delete resync')
  await expect(page.getByRole('alert').filter({ hasText: '在最新同步中已不存在' })).toBeVisible()
  await expect(page.getByRole('button', { name: '保存渠道' })).toBeDisabled()
  await expect(ruleNameInput).toHaveValue('In-App Global Rule Newer')

  // 9. 401 session expiry while saving a dirty rule exits cleanly to /login without a discard prompt.
  await page.getByLabel('确认改为应用于所有活动节点').check()
  expireRuleSaveWith401 = true
  await page.getByRole('button', { name: '保存规则' }).click()
  await expect(page).toHaveURL(/\/login\?redirect=/)
  await expect(discardDialog).toBeHidden()
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
})
