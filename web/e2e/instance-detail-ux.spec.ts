import type { Locator } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'
import { login } from './auth'

const MISSING_V6_NODE_ID = '00000000-0000-4000-8000-000000000334'
const CHANGED_V6_NODE_ID = '00000000-0000-4000-8000-000000000335'
const TOUCH_SCOPE_NODE_ID = '00000000-0000-4000-8000-000000000336'
const SWITCH_AUDIT_NODE_ID = '00000000-0000-4000-8000-000000000337'

async function activateControl(locator: Locator, hasTouch: boolean) {
  if (hasTouch)
    await locator.tap()
  else
    await locator.click()
}

function makeUnlockServices(region: string) {
  return [
    { id: 'youtube', status: 'unlocked', region },
    { id: 'netflix', status: 'unlocked', region },
    { id: 'disney_plus', status: 'unlocked', region },
    { id: 'chatgpt', status: 'unlocked', region },
    { id: 'reddit', status: 'unlocked', region },
    { id: 'tiktok', status: 'unlocked', region },
  ]
}

function makeStatusRecord(uuid: string, timestampMs: number, cpu: number) {
  return {
    client: uuid,
    time: new Date(timestampMs).toISOString(),
    cpu,
    gpu: null,
    ram: 512 * 1024 * 1024,
    ram_total: 2 * 1024 * 1024 * 1024,
    swap: 0,
    swap_total: 512 * 1024 * 1024,
    load: 0.35,
    load5: 0.31,
    load15: 0.28,
    temp: null,
    disk: 8 * 1024 * 1024 * 1024,
    disk_total: 40 * 1024 * 1024 * 1024,
    net_in: 128 * 1024,
    net_out: 64 * 1024,
    net_total_up: 4 * 1024 * 1024 * 1024,
    net_total_down: 9 * 1024 * 1024 * 1024,
    process: 96,
    connections: 42,
    connections_udp: 6,
  }
}

test('instance detail keeps unlock card stable for missing IPv6 and address_changed while hiding invalid Google badge', async ({ page }) => {
  const checkedAt = Date.UTC(2026, 4, 10, 11, 30, 0)
  await page.route('**/api/rpc2', async (route) => {
    const body = route.request().postDataJSON() as {
      id?: number
      method?: string
      params?: { uuid?: string }
    } | null
    if (body?.method === 'common:getNodeRecentStatus') {
      const uuid = body.params?.uuid ?? MISSING_V6_NODE_ID
      await route.fulfill({
        json: {
          jsonrpc: '2.0',
          id: body.id,
          result: {
            count: 2,
            records: [
              makeStatusRecord(uuid, checkedAt - 3_600_000, 15.2),
              makeStatusRecord(uuid, checkedAt, 19.4),
            ],
            coverage: {
              requested_start_unix_ms: checkedAt - 4 * 3_600_000,
              requested_end_unix_ms: checkedAt,
              actual_start_unix_ms: checkedAt - 3_600_000,
              actual_end_unix_ms: checkedAt,
              source_points: 2,
              returned_points: 2,
              bucket_ms: 0,
              downsampled: false,
            },
          },
        },
      })
      return
    }
    const response = await route.fetch()
    if (body?.method !== 'common:getDashboard') {
      await route.fulfill({ response })
      return
    }
    const payload = await response.json() as {
      result?: {
        clients?: Record<string, Record<string, unknown>>
        statuses?: Record<string, Record<string, unknown>>
      }
    }
    const clients = payload.result?.clients ?? {}
    const statuses = payload.result?.statuses ?? {}
    const templateEntry = Object.entries(clients)[0]
    if (templateEntry) {
      const [templateId, templateClient] = templateEntry
      const templateStatus = statuses[templateId]

      clients[MISSING_V6_NODE_ID] = {
        ...templateClient,
        uuid: MISSING_V6_NODE_ID,
        name: 'unlock-missing-v6',
        region: 'JP',
        ipv4: '198.51.100.10',
        ipv6: '2001:db8::10',
        ip_unlocks: [
          {
            family: 4,
            checked_at_unix_ms: checkedAt,
            address_changed: false,
            google: { status: 'normal', region: 'JP' },
            services: makeUnlockServices('JP'),
          },
        ],
      }

      clients[CHANGED_V6_NODE_ID] = {
        ...templateClient,
        uuid: CHANGED_V6_NODE_ID,
        name: 'unlock-changed-v6',
        region: 'US',
        ipv4: '198.51.100.20',
        ipv6: '2001:db8::20',
        ip_unlocks: [
          {
            family: 4,
            checked_at_unix_ms: checkedAt,
            address_changed: false,
            google: { status: 'normal', region: 'US' },
            services: makeUnlockServices('US'),
          },
          {
            family: 6,
            checked_at_unix_ms: checkedAt,
            address_changed: true,
            google: { status: 'normal', region: 'US' },
            services: makeUnlockServices('US').slice(0, 5),
          },
        ],
      }

      if (templateStatus) {
        statuses[MISSING_V6_NODE_ID] = {
          ...templateStatus,
          client: MISSING_V6_NODE_ID,
        }
        statuses[CHANGED_V6_NODE_ID] = {
          ...templateStatus,
          client: CHANGED_V6_NODE_ID,
        }
      }
    }
    await route.fulfill({ response, json: payload })
  })

  await login(page)
  const missingNodeLink = page.getByRole('link', { name: '查看 unlock-missing-v6 节点详情' })
  await expect(missingNodeLink).toBeVisible({ timeout: 20_000 })
  await missingNodeLink.click()
  await expect(page).toHaveURL(new RegExp(`/instance/${MISSING_V6_NODE_ID}$`))

  const unlockPanel = page.getByTestId('unlock-summary-panel')
  await expect(page.getByText('Google 未送中 (JP)', { exact: true })).toBeVisible()
  await expect(unlockPanel).toContainText('流媒体与 AI 解锁 · IPv4')
  await expect(unlockPanel).toContainText('YouTube')

  await page.getByRole('button', { name: 'unlock-missing-v6 显示 IPv6' }).click()
  await expect(page.getByText(/^Google /)).toHaveCount(0)
  await expect(unlockPanel).toBeVisible()
  await expect(unlockPanel).toContainText('流媒体与 AI 解锁 · IPv6')
  await expect(unlockPanel).toContainText('暂无记录')
  await expect(unlockPanel).toContainText('暂无 IPv6 流媒体与 AI 解锁记录。')
  await expect(unlockPanel.locator('[data-unlock-state="empty"]')).toBeVisible()
  await expect(unlockPanel).not.toContainText('回程')

  const emptyAccessibility = await new AxeBuilder({ page })
    .include('[data-testid="unlock-summary-panel"]')
    .analyze()
  expect(emptyAccessibility.violations.filter(({ impact }) => impact === 'serious' || impact === 'critical')).toEqual([])

  await page.getByRole('button', { name: 'unlock-missing-v6 显示 IPv4' }).click()
  await expect(page.getByText('Google 未送中 (JP)', { exact: true })).toBeVisible()
  await expect(unlockPanel).toContainText('流媒体与 AI 解锁 · IPv4')
  await expect(unlockPanel).toContainText('TikTok')

  await page.goto(`/instance/${CHANGED_V6_NODE_ID}`)
  await expect(page.getByText('Google 未送中 (US)', { exact: true })).toBeVisible()
  await expect(unlockPanel).toContainText('流媒体与 AI 解锁 · IPv4')

  await page.getByRole('button', { name: 'unlock-changed-v6 显示 IPv6' }).click()
  await expect(page.getByText(/^Google /)).toHaveCount(0)
  await expect(unlockPanel).toBeVisible()
  await expect(unlockPanel).toContainText('流媒体与 AI 解锁 · IPv6')
  await expect(unlockPanel).toContainText('地址已变更')
  await expect(unlockPanel).toContainText('旧的流媒体与 AI 解锁结果不再适用于当前地址')
  await expect(unlockPanel.locator('[data-unlock-state="address_changed"]')).toBeVisible()
  await expect(unlockPanel).not.toContainText('暂无记录')

  const changedAccessibility = await new AxeBuilder({ page })
    .include('[data-testid="unlock-summary-panel"]')
    .analyze()
  expect(changedAccessibility.violations.filter(({ impact }) => impact === 'serious' || impact === 'critical')).toEqual([])
})

test('load chart shows response-driven coverage window and refresh status without stale leaks', async ({ page }) => {
  const windowStart = Date.UTC(2026, 4, 10, 8, 0, 0)
  const windowEnd = Date.UTC(2026, 4, 10, 12, 0, 0)
  let releaseFourHourResponse: (() => void) | undefined

  await page.route('**/api/rpc2', async (route) => {
    const body = route.request().postDataJSON() as {
      id?: number
      method?: string
      params?: { uuid?: string, hours?: number, type?: string }
    } | null
    if (body?.method === 'common:getNodeRecentStatus') {
      const uuid = body.params?.uuid ?? 'e2e-node'
      await route.fulfill({
        json: {
          jsonrpc: '2.0',
          id: body.id,
          result: {
            count: 2,
            records: [
              makeStatusRecord(uuid, windowStart, 18.5),
              makeStatusRecord(uuid, windowEnd, 24.2),
            ],
            coverage: {
              requested_start_unix_ms: windowStart,
              requested_end_unix_ms: windowEnd,
              actual_start_unix_ms: windowStart,
              actual_end_unix_ms: windowEnd,
              source_points: 2,
              returned_points: 2,
              bucket_ms: 0,
              downsampled: false,
            },
          },
        },
      })
      return
    }

    if (body?.method === 'common:getRecords' && body.params?.type === 'load') {
      const uuid = body.params?.uuid ?? 'e2e-node'
      const hours = body.params?.hours ?? 4
      if (hours === 4) {
        await new Promise<void>((resolve) => {
          releaseFourHourResponse = resolve
        })
        await route.fulfill({
          json: {
            jsonrpc: '2.0',
            id: body.id,
            result: {
              count: 2,
              records: [
                makeStatusRecord(uuid, windowStart, 18.5),
                makeStatusRecord(uuid, windowEnd, 24.2),
              ],
              coverage: {
                requested_start_unix_ms: windowStart,
                requested_end_unix_ms: windowEnd,
                actual_start_unix_ms: windowStart,
                actual_end_unix_ms: windowEnd,
                source_points: 240,
                returned_points: 240,
                bucket_ms: 60_000,
                downsampled: false,
              },
            },
          },
        })
        return
      }
      if (hours === 24) {
        await route.fulfill({
          json: {
            jsonrpc: '2.0',
            id: body.id,
            result: {
              count: 0,
              records: [],
              coverage: {
                requested_start_unix_ms: windowEnd - 24 * 3_600_000,
                requested_end_unix_ms: windowEnd,
                actual_start_unix_ms: null,
                actual_end_unix_ms: null,
                source_points: 0,
                returned_points: 0,
                bucket_ms: 0,
                downsampled: false,
              },
            },
          },
        })
        return
      }
      if (hours === 168) {
        await route.fulfill({
          json: {
            jsonrpc: '2.0',
            id: body.id,
            error: {
              code: -32000,
              message: '模拟历史读取失败',
            },
          },
        })
        return
      }
    }

    await route.continue()
  })

  await login(page)
  const nodeLink = page.getByRole('link', { name: '查看 e2e-node 节点详情' })
  await expect(nodeLink).toBeVisible({ timeout: 20_000 })
  await nodeLink.click()
  await expect(page).toHaveURL(/\/instance\/[0-9a-f-]+$/)

  const expectedRange = await page.evaluate(([startMs, endMs]) => {
    const pad = (value: number) => String(value).padStart(2, '0')
    const start = new Date(startMs)
    const end = new Date(endMs)
    const sameDay = start.getFullYear() === end.getFullYear()
      && start.getMonth() === end.getMonth()
      && start.getDate() === end.getDate()
    const startPrefix = `${pad(start.getMonth() + 1)}/${pad(start.getDate())} ${pad(start.getHours())}:${pad(start.getMinutes())}`
    const endPart = sameDay
      ? `${pad(end.getHours())}:${pad(end.getMinutes())}`
      : `${pad(end.getMonth() + 1)}/${pad(end.getDate())} ${pad(end.getHours())}:${pad(end.getMinutes())}`
    return `${startPrefix} – ${endPart}`
  }, [windowStart, windowEnd] as const)

  const statusBar = page.getByTestId('load-chart-status')
  const modeBadge = page.getByTestId('load-chart-mode')
  const coverageText = page.getByTestId('load-chart-coverage')

  await expect(modeBadge).toHaveText('自动刷新中')
  await expect(coverageText).toHaveText(`覆盖时段 ${expectedRange}`)
  await expect(statusBar).not.toHaveAttribute('role', 'status')
  await expect(statusBar).not.toHaveAttribute('aria-live', /.+/)

  const rangeList = page.getByRole('tablist', { name: '资源负载时间范围' })
  const fourHourTab = rangeList.getByRole('tab', { name: '4 小时', exact: true })
  await fourHourTab.click()
  await expect.poll(() => releaseFourHourResponse !== undefined).toBe(true)
  await expect(modeBadge).toHaveText('历史快照')
  await expect(coverageText).toHaveText('正在读取覆盖时段…')
  await expect(coverageText).not.toContainText(expectedRange)

  releaseFourHourResponse?.()
  await expect(modeBadge).toHaveText('历史快照')
  await expect(coverageText).toHaveText(`覆盖时段 ${expectedRange}`)

  await rangeList.getByRole('tab', { name: '1 天', exact: true }).click()
  await expect(modeBadge).toHaveText('历史快照')
  await expect(coverageText).toHaveText('当前范围暂无采样数据')
  await expect(coverageText).not.toContainText(expectedRange)

  await rangeList.getByRole('tab', { name: '7 天', exact: true }).click()
  await expect(modeBadge).toHaveText('历史快照')
  await expect(coverageText).toHaveText('暂未获取覆盖时段')
  await expect(coverageText).not.toContainText(expectedRange)
  await expect(page.getByText('模拟历史读取失败')).toBeVisible()

  await fourHourTab.focus()
  await page.keyboard.press('ArrowLeft')
  await expect(rangeList.getByRole('tab', { name: '实时', exact: true })).toHaveAttribute('data-state', 'active')
  await expect(modeBadge).toHaveText('自动刷新中')
})

test('mobile detail touch targets reach 44px with compact 26px network pills and safe probe separation without leaking to home or modals', async ({ page }, testInfo) => {
  const hasTouch = testInfo.project.use.hasTouch === true
  await page.setViewportSize({ width: 375, height: 812 })
  const checkedAt = Date.UTC(2026, 4, 10, 11, 30, 0)
  const activeProbeRequests: string[] = []

  await page.route('**/api/admin/**', async (route) => {
    if (route.request().method() === 'POST')
      activeProbeRequests.push(route.request().url())
    await route.continue()
  })

  await page.route('**/api/public', async (route) => {
    const response = await route.fetch()
    const payload = await response.json()
    payload.data.record_preserve_time = 2400
    await route.fulfill({ response, json: payload })
  })

  await page.route('**/api/rpc2', async (route) => {
    const body = route.request().postDataJSON() as {
      id?: number
      method?: string
      params?: { uuid?: string, type?: string }
    } | null
    if (body?.method === 'common:getNodeRecentStatus' || (body?.method === 'common:getRecords' && body.params?.type === 'load')) {
      const uuid = body.params?.uuid ?? TOUCH_SCOPE_NODE_ID
      await route.fulfill({
        json: {
          jsonrpc: '2.0',
          id: body.id,
          result: {
            count: 2,
            records: [
              makeStatusRecord(uuid, checkedAt - 3_600_000, 16.5),
              makeStatusRecord(uuid, checkedAt, 22.1),
            ],
            coverage: {
              requested_start_unix_ms: checkedAt - 4 * 3_600_000,
              requested_end_unix_ms: checkedAt,
              actual_start_unix_ms: checkedAt - 3_600_000,
              actual_end_unix_ms: checkedAt,
              source_points: 2,
              returned_points: 2,
              bucket_ms: 0,
              downsampled: false,
            },
          },
        },
      })
      return
    }
    const response = await route.fetch()
    if (body?.method !== 'common:getDashboard') {
      await route.fulfill({ response })
      return
    }
    const payload = await response.json() as {
      result?: {
        clients?: Record<string, Record<string, unknown>>
        statuses?: Record<string, Record<string, unknown>>
      }
    }
    const clients = payload.result?.clients ?? {}
    const statuses = payload.result?.statuses ?? {}
    const templateEntry = Object.entries(clients)[0]
    if (templateEntry) {
      const [templateId, templateClient] = templateEntry
      const templateStatus = statuses[templateId]

      clients[TOUCH_SCOPE_NODE_ID] = {
        ...templateClient,
        uuid: TOUCH_SCOPE_NODE_ID,
        name: 'touch-scope-node',
        region: 'SG',
        ipv4: '198.51.100.30',
        ipv6: '2001:db8::30',
        return_route_capability: 'ready',
        ip_scores: [
          {
            family: 4,
            score: 92,
            provider: 'Net.Coffee',
            updated_at_unix_ms: checkedAt,
          },
        ],
        ip_unlocks: [
          {
            family: 4,
            checked_at_unix_ms: checkedAt,
            address_changed: false,
            google: { status: 'normal', region: 'SG' },
            services: makeUnlockServices('SG'),
          },
        ],
        return_routes: [
          {
            family: 4,
            region: '上海',
            protocol: 'TCP',
            source: 'agent',
            checked_at_unix_ms: checkedAt,
            address_changed: false,
            carriers: [
              {
                carrier: 'telecom',
                target: '198.51.100.1',
                status: 'ok',
                label: 'CN2 GIA',
              },
              {
                carrier: 'unicom',
                target: '198.51.100.2',
                status: 'ok',
                label: '9929',
              },
              {
                carrier: 'mobile',
                target: '198.51.100.3',
                status: 'ok',
                label: 'CMIN2',
              },
            ],
          },
        ],
      }

      if (templateStatus) {
        statuses[TOUCH_SCOPE_NODE_ID] = {
          ...templateStatus,
          client: TOUCH_SCOPE_NODE_ID,
        }
      }
    }
    await route.fulfill({ response, json: payload })
  })

  await login(page)
  const cardLink = page.getByRole('link', { name: '查看 touch-scope-node 节点详情' })
  await expect(cardLink).toBeVisible({ timeout: 20_000 })
  const card = page.getByTestId('node-card').filter({ has: cardLink })

  // Home card compact switch remains compact (no leak).
  const compactToggle = card.getByRole('button', { name: '当前 IPv4，切换 IP 版本' })
  await expect(compactToggle).toBeVisible()
  const compactToggleBox = await compactToggle.boundingBox()
  expect(compactToggleBox?.height ?? 999).toBeLessThanOrEqual(24)

  // IP quality modal switch remains compact (no leak).
  await card.getByRole('button', { name: /touch-scope-node IPv4 IP 评分 .*查看详情/ }).click()
  const ipDialog = page.getByRole('dialog', { name: 'IP 质量' })
  await expect(ipDialog).toBeVisible()
  const ipDialogV4Box = await ipDialog.getByRole('button', { name: 'touch-scope-node 显示 IPv4' }).boundingBox()
  expect(ipDialogV4Box?.height ?? 999).toBeLessThan(32)
  await page.keyboard.press('Escape')
  await expect(ipDialog).toBeHidden()

  // Return routes modal controls remain compact (no leak).
  await card.getByRole('button', { name: /touch-scope-node IPv4 三网回程：.*查看详情/ }).click()
  const routesDialog = page.getByRole('dialog', { name: '三网回程' })
  await expect(routesDialog).toBeVisible()
  const routesDialogV4Box = await routesDialog.getByRole('button', { name: 'touch-scope-node 显示 IPv4' }).boundingBox()
  const routesDialogDetectBox = await routesDialog.getByRole('button', { name: 'touch-scope-node 检测 IPv4 网络质量' }).boundingBox()
  expect(routesDialogV4Box?.height ?? 999).toBeLessThan(32)
  expect(routesDialogDetectBox?.height ?? 999).toBeLessThan(34)
  await page.keyboard.press('Escape')
  await expect(routesDialog).toBeHidden()

  // Navigate to instance detail at 375px and verify 44px touch targets with ~32px visual capsules.
  await activateControl(cardLink, hasTouch)
  await expect(page).toHaveURL(new RegExp(`/instance/${TOUCH_SCOPE_NODE_ID}$`))

  const detailIpGroup = page.getByRole('group', { name: 'touch-scope-node IP 版本' })
  const detailV4 = detailIpGroup.getByRole('button', { name: 'touch-scope-node 显示 IPv4' })
  const detailV6 = detailIpGroup.getByRole('button', { name: 'touch-scope-node 显示 IPv6' })
  const detailTrack = detailIpGroup.getByTestId('ip-version-track')
  const detailIndicator = detailIpGroup.getByTestId('ip-version-indicator')
  const detailDetect = page.getByRole('button', { name: 'touch-scope-node 检测 IPv4 网络质量' })
  const detailDetectPill = detailDetect.getByTestId('network-detect-pill')

  await expect(detailV4).toBeVisible()
  await expect(detailDetect).toBeVisible()

  const detailV4Box = await detailV4.boundingBox()
  const detailV6Box = await detailV6.boundingBox()
  const detailTrackBox = await detailTrack.boundingBox()
  const detailIndicatorBox = await detailIndicator.boundingBox()
  const detailDetectBox = await detailDetect.boundingBox()
  const detailDetectPillBox = await detailDetectPill.boundingBox()

  expect(detailV4Box?.height ?? 0).toBeGreaterThanOrEqual(44)
  expect(detailV4Box?.width ?? 0).toBeGreaterThanOrEqual(44)
  expect(detailV4Box?.width ?? 999).toBeLessThanOrEqual(48)
  expect(detailV6Box?.height ?? 0).toBeGreaterThanOrEqual(44)
  expect(detailV6Box?.width ?? 0).toBeGreaterThanOrEqual(44)
  expect(detailV6Box?.width ?? 999).toBeLessThanOrEqual(48)
  expect(Math.abs((detailV4Box?.width ?? 0) - (detailV6Box?.width ?? 0))).toBeLessThanOrEqual(2)
  expect((detailV4Box?.x ?? 0) + (detailV4Box?.width ?? 0)).toBeLessThanOrEqual((detailV6Box?.x ?? 0) + 1)
  expect(detailTrackBox?.height ?? 0).toBeGreaterThanOrEqual(24)
  expect(detailTrackBox?.height ?? 999).toBeLessThanOrEqual(28)
  expect(detailTrackBox?.width ?? 0).toBeGreaterThanOrEqual(84)
  expect(detailTrackBox?.width ?? 999).toBeLessThanOrEqual(94)
  expect(detailIndicatorBox?.height ?? 0).toBeGreaterThanOrEqual(20)
  expect(detailIndicatorBox?.height ?? 999).toBeLessThanOrEqual(24)
  expect(detailDetectBox?.height ?? 0).toBeGreaterThanOrEqual(44)
  expect(detailDetectBox?.width ?? 0).toBeGreaterThanOrEqual(44)
  expect(detailDetectPillBox?.height ?? 0).toBeGreaterThanOrEqual(24)
  expect(detailDetectPillBox?.height ?? 999).toBeLessThanOrEqual(28)
  expect(detailDetectPillBox?.width ?? 999).toBeLessThanOrEqual(80)
  expect(Math.abs((detailTrackBox?.height ?? 0) - (detailDetectPillBox?.height ?? 0))).toBeLessThanOrEqual(1)
  expect((detailDetectBox?.x ?? 0) - ((detailV6Box?.x ?? 0) + (detailV6Box?.width ?? 0))).toBeGreaterThanOrEqual(12)

  // Rapid IPv6 -> IPv4 switching keeps single active selection and synced family data without triggering probes.
  const unlockPanel = page.getByTestId('unlock-summary-panel')
  await activateControl(detailV6, hasTouch)
  await expect(detailV6).toHaveAttribute('aria-pressed', 'true')
  await expect(detailV4).toHaveAttribute('aria-pressed', 'false')
  await expect(unlockPanel).toContainText('流媒体与 AI 解锁 · IPv6')
  await expect(page.getByRole('button', { name: /touch-scope-node IPv6 三网回程：/ })).toBeVisible()

  await activateControl(detailV4, hasTouch)
  await expect(detailV4).toHaveAttribute('aria-pressed', 'true')
  await expect(detailV6).toHaveAttribute('aria-pressed', 'false')
  await expect(unlockPanel).toContainText('流媒体与 AI 解锁 · IPv4')
  await expect(page.getByRole('button', { name: /touch-scope-node IPv4 三网回程：.*CN2 GIA/ })).toBeVisible()

  // LoadChart time-range tabs reach 44px touch / ~32px visual pill and handle 100 天 without horizontal overflow.
  const rangeList = page.getByRole('tablist', { name: '资源负载时间范围' })
  await expect(rangeList.getByRole('tab', { name: '100 天', exact: true })).toBeVisible()
  const tabs = rangeList.getByRole('tab')
  const tabCount = await tabs.count()
  expect(tabCount).toBe(6)
  for (let index = 0; index < tabCount; index += 1) {
    const tab = tabs.nth(index)
    const box = await tab.boundingBox()
    const pillBox = await tab.getByTestId('load-tab-pill').boundingBox()
    expect(box?.height ?? 0).toBeGreaterThanOrEqual(44)
    expect(box?.width ?? 0).toBeGreaterThanOrEqual(44)
    expect(pillBox?.height ?? 0).toBeGreaterThanOrEqual(30)
    expect(pillBox?.height ?? 999).toBeLessThanOrEqual(34)
  }

  const noPageOverflow = await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)
  expect(noPageOverflow).toBe(true)

  const hundredDaysTab = rangeList.getByRole('tab', { name: '100 天', exact: true })
  await hundredDaysTab.scrollIntoViewIfNeeded()
  await activateControl(hundredDaysTab, hasTouch)
  await expect(hundredDaysTab).toHaveAttribute('data-state', 'active')
  await expect(rangeList.locator('[role="tab"][data-state="active"]')).toHaveCount(1)
  const activePillBg = await hundredDaysTab.getByTestId('load-tab-pill').evaluate(element => getComputedStyle(element).backgroundColor)
  expect(activePillBg).not.toBe('rgba(0, 0, 0, 0)')

  // Modals opened from detail page still keep their compact controls (no leak).
  await page.getByRole('button', { name: /touch-scope-node IPv4 三网回程：.*查看详情/ }).click()
  await expect(routesDialog).toBeVisible()
  const detailRoutesDialogV4Box = await routesDialog.getByRole('button', { name: 'touch-scope-node 显示 IPv4' }).boundingBox()
  const detailRoutesDialogDetectBox = await routesDialog.getByRole('button', { name: 'touch-scope-node 检测 IPv4 网络质量' }).boundingBox()
  expect(detailRoutesDialogV4Box?.height ?? 999).toBeLessThan(32)
  expect(detailRoutesDialogDetectBox?.height ?? 999).toBeLessThan(34)
  await page.keyboard.press('Escape')
  await expect(routesDialog).toBeHidden()

  // Desktop viewport restores compact sizing on detail controls.
  await page.setViewportSize({ width: 1280, height: 720 })
  await expect.poll(async () => (await detailV4.boundingBox())?.height ?? 999).toBeLessThan(32)
  await expect.poll(async () => (await rangeList.getByRole('tab', { name: '实时', exact: true }).boundingBox())?.height ?? 999).toBeLessThan(34)
  expect(activeProbeRequests).toEqual([])
})

test('detail switches, hover states, probe highlight, and tooltip cleanup work across mouse, keyboard, and touch', async ({ page }, testInfo) => {
  const hasTouch = testInfo.project.use.hasTouch === true
  const checkedAt = Date.UTC(2026, 4, 10, 11, 30, 0)
  const activeProbeRequests: string[] = []

  if (hasTouch)
    await page.setViewportSize({ width: 375, height: 812 })

  await page.route('**/api/admin/**', async (route) => {
    if (route.request().method() === 'POST')
      activeProbeRequests.push(route.request().url())
    await route.continue()
  })

  await page.route('**/api/public', async (route) => {
    const response = await route.fetch()
    const payload = await response.json()
    payload.data.record_preserve_time = 2400
    payload.data.ping_record_preserve_time = 2400
    await route.fulfill({ response, json: payload })
  })

  await page.route('**/api/rpc2', async (route) => {
    const body = route.request().postDataJSON() as {
      id?: number
      method?: string
      params?: { uuid?: string, type?: string }
    } | null
    if (body?.method === 'common:getNodeRecentStatus' || (body?.method === 'common:getRecords' && body.params?.type === 'load')) {
      const uuid = body.params?.uuid ?? SWITCH_AUDIT_NODE_ID
      await route.fulfill({
        json: {
          jsonrpc: '2.0',
          id: body.id,
          result: {
            count: 2,
            records: [
              makeStatusRecord(uuid, checkedAt - 3_600_000, 15.0),
              makeStatusRecord(uuid, checkedAt, 20.5),
            ],
            coverage: {
              requested_start_unix_ms: checkedAt - 4 * 3_600_000,
              requested_end_unix_ms: checkedAt,
              actual_start_unix_ms: checkedAt - 3_600_000,
              actual_end_unix_ms: checkedAt,
              source_points: 2,
              returned_points: 2,
              bucket_ms: 0,
              downsampled: false,
            },
          },
        },
      })
      return
    }
    const response = await route.fetch()
    if (body?.method !== 'common:getDashboard') {
      await route.fulfill({ response })
      return
    }
    const payload = await response.json() as {
      result?: {
        clients?: Record<string, Record<string, unknown>>
        statuses?: Record<string, Record<string, unknown>>
      }
    }
    const clients = payload.result?.clients ?? {}
    const statuses = payload.result?.statuses ?? {}
    const templateEntry = Object.entries(clients)[0]
    if (templateEntry) {
      const [templateId, templateClient] = templateEntry
      const templateStatus = statuses[templateId]

      clients[SWITCH_AUDIT_NODE_ID] = {
        ...templateClient,
        uuid: SWITCH_AUDIT_NODE_ID,
        name: 'switch-audit-node',
        region: 'JP',
        ipv4: '198.51.100.40',
        ipv6: '2001:db8::40',
        return_route_capability: 'ready',
        ip_unlocks: [
          {
            family: 4,
            checked_at_unix_ms: checkedAt,
            address_changed: false,
            google: { status: 'normal', region: 'JP' },
            services: makeUnlockServices('JP'),
          },
        ],
      }

      if (templateStatus) {
        statuses[SWITCH_AUDIT_NODE_ID] = {
          ...templateStatus,
          client: SWITCH_AUDIT_NODE_ID,
        }
      }
    }
    await route.fulfill({ response, json: payload })
  })

  await page.route('**/api/v1/nodes/*/probes?*', async (route) => {
    const now = Date.now()
    await route.fulfill({
      json: {
        tasks: [
          {
            id: 'probe-v4',
            name: 'Tokyo-V4',
            kind: 'tcp',
            interval_seconds: 60,
          },
          {
            id: 'probe-v6',
            name: 'Tokyo-IPv6',
            kind: 'tcp',
            interval_seconds: 60,
          },
        ],
        records: [
          {
            task_id: 'probe-v4',
            collected_at_unix_ms: now,
            received_at_unix_ms: now,
            latency_ms: 20,
            success: true,
            error: null,
          },
        ],
        chart: [
          {
            task_id: 'probe-v4',
            received_at_unix_ms: now,
            latency_ms: 20,
            failed_samples: 0,
            samples: 1,
          },
          {
            task_id: 'probe-v6',
            received_at_unix_ms: now,
            latency_ms: 35,
            failed_samples: 1,
            samples: 2,
          },
        ],
        summary: [
          {
            task_id: 'probe-v4',
            samples: 4,
            loss_percent: 0,
            avg_latency_ms: 20,
            min_latency_ms: 18,
            max_latency_ms: 22,
            latest_latency_ms: 20,
            p50_latency_ms: 20,
            p95_latency_ms: 22,
            p99_latency_ms: 22,
            p99_p50_ratio: 0.1,
          },
          {
            task_id: 'probe-v6',
            samples: 2,
            loss_percent: 50,
            avg_latency_ms: 35,
            min_latency_ms: 35,
            max_latency_ms: 35,
            latest_latency_ms: 35,
            p50_latency_ms: 35,
            p95_latency_ms: 35,
            p99_latency_ms: 35,
            p99_p50_ratio: 0,
          },
        ],
        chart_source_points: 2,
        chart_bucket_ms: 3601,
        range_start_unix_ms: now - 3_600_000,
        range_end_unix_ms: now,
      },
    })
  })

  await login(page)
  const auditNodeLink = page.getByRole('link', { name: '查看 switch-audit-node 节点详情' })
  await expect(auditNodeLink).toBeVisible({ timeout: 20_000 })
  await activateControl(auditNodeLink, hasTouch)
  await expect(page).toHaveURL(new RegExp(`/instance/${SWITCH_AUDIT_NODE_ID}$`))

  const ipGroup = page.getByRole('group', { name: 'switch-audit-node IP 版本' })
  const v4Btn = ipGroup.getByRole('button', { name: 'switch-audit-node 显示 IPv4' })
  const v6Btn = ipGroup.getByRole('button', { name: 'switch-audit-node 显示 IPv6' })
  const unlockPanel = page.getByTestId('unlock-summary-panel')

  const loadRangeList = page.getByRole('tablist', { name: '资源负载时间范围' })
  const probeRangeList = page.getByRole('tablist', { name: '探测历史时间范围' })
  const metricGroup = page.getByRole('group', { name: '趋势指标' })
  const presetGroup = page.getByRole('group', { name: '线路快捷筛选' })
  const trendSection = page.locator('section[aria-label="线路趋势"]')
  const focusV4Btn = page.getByRole('button', { name: '仅看 Tokyo-V4' })

  await expect(probeRangeList.getByRole('tab', { name: '1 小时', exact: true })).toBeVisible()
  await expect(focusV4Btn).toBeVisible()

  if (hasTouch) {
    // Touch: rapid tap IPv6 -> IPv4 stays synced without triggering probes.
    await v6Btn.tap()
    await expect(v6Btn).toHaveAttribute('aria-pressed', 'true')
    await expect(v4Btn).toHaveAttribute('aria-pressed', 'false')
    await expect(unlockPanel).toContainText('流媒体与 AI 解锁 · IPv6')
    await v4Btn.tap()
    await expect(v4Btn).toHaveAttribute('aria-pressed', 'true')
    await expect(v6Btn).toHaveAttribute('aria-pressed', 'false')
    await expect(unlockPanel).toContainText('流媒体与 AI 解锁 · IPv4')

    // Touch: ProbeHistory time tabs keep 44px hit / ~32px pill and zero horizontal page overflow.
    const probeTabs = probeRangeList.getByRole('tab')
    const probeTabCount = await probeTabs.count()
    expect(probeTabCount).toBeGreaterThanOrEqual(6)
    for (let index = 0; index < probeTabCount; index += 1) {
      const tab = probeTabs.nth(index)
      const box = await tab.boundingBox()
      const pillBox = await tab.getByTestId('probe-tab-pill').boundingBox()
      expect(box?.height ?? 0).toBeGreaterThanOrEqual(44)
      expect(pillBox?.height ?? 0).toBeGreaterThanOrEqual(30)
      expect(pillBox?.height ?? 999).toBeLessThanOrEqual(34)
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)

    const probeSixHour = probeRangeList.getByRole('tab', { name: '6 小时', exact: true })
    await probeSixHour.tap()
    await expect(probeSixHour).toHaveAttribute('data-state', 'active')
    await expect(probeRangeList.locator('[role="tab"][data-state="active"]')).toHaveCount(1)
    expect(await probeSixHour.getByTestId('probe-tab-pill').evaluate(element => getComputedStyle(element).backgroundColor)).not.toBe('rgba(0, 0, 0, 0)')

    await metricGroup.getByRole('button', { name: '丢包', exact: true }).tap()
    await expect(metricGroup.locator('button[aria-pressed="true"]')).toHaveCount(1)
    await metricGroup.getByRole('button', { name: 'RTT', exact: true }).tap()
    await expect(metricGroup.locator('button[aria-pressed="true"]')).toHaveCount(1)

    await presetGroup.getByRole('button', { name: 'V4', exact: true }).tap()
    await expect(presetGroup.locator('button[aria-pressed="true"]')).toHaveCount(1)
    await presetGroup.getByRole('button', { name: '全部', exact: true }).tap()
    await expect(presetGroup.locator('button[aria-pressed="true"]')).toHaveCount(1)

    // Touch tap on line inspector focuses and restores cleanly WITHOUT sticking hover/focus highlight.
    await focusV4Btn.tap()
    await expect(focusV4Btn).toHaveAttribute('aria-pressed', 'true')
    await expect(trendSection).not.toHaveAttribute('data-highlighted-task', /.+/)
    await focusV4Btn.tap()
    await expect(focusV4Btn).toHaveAttribute('aria-pressed', 'false')
    await expect(trendSection).not.toHaveAttribute('data-highlighted-task', /.+/)
  }
  else {
    // Desktop: hover & keyboard focus must NEVER change IPv4/IPv6 selection.
    await v6Btn.hover()
    await expect(v4Btn).toHaveAttribute('aria-pressed', 'true')
    await expect(v6Btn).toHaveAttribute('aria-pressed', 'false')
    await page.mouse.move(0, 0)

    await v6Btn.focus()
    await expect(v4Btn).toHaveAttribute('aria-pressed', 'true')
    await expect(v6Btn).toHaveAttribute('aria-pressed', 'false')
    await page.keyboard.press('Space')
    await expect(v6Btn).toHaveAttribute('aria-pressed', 'true')
    await expect(unlockPanel).toContainText('流媒体与 AI 解锁 · IPv6')
    await v4Btn.focus()
    await page.keyboard.press('Enter')
    await expect(v4Btn).toHaveAttribute('aria-pressed', 'true')
    await expect(unlockPanel).toContainText('流媒体与 AI 解锁 · IPv4')

    // Desktop: active LoadChart and ProbeHistory tabs show visible active background pill.
    const loadActiveBg = await loadRangeList.getByRole('tab', { name: '实时', exact: true }).evaluate(element => getComputedStyle(element).backgroundColor)
    expect(loadActiveBg).not.toBe('rgba(0, 0, 0, 0)')

    const probeOneHour = probeRangeList.getByRole('tab', { name: '1 小时', exact: true })
    const probeSixHour = probeRangeList.getByRole('tab', { name: '6 小时', exact: true })
    expect(await probeOneHour.evaluate(element => getComputedStyle(element).backgroundColor)).not.toBe('rgba(0, 0, 0, 0)')
    await probeSixHour.click()
    await expect(probeSixHour).toHaveAttribute('data-state', 'active')
    expect(await probeSixHour.evaluate(element => getComputedStyle(element).backgroundColor)).not.toBe('rgba(0, 0, 0, 0)')
    expect(await probeOneHour.evaluate(element => getComputedStyle(element).backgroundColor)).toBe('rgba(0, 0, 0, 0)')

    // Desktop: hovering an already-selected preset button keeps its settled active pill background intact.
    const presetV4 = presetGroup.getByRole('button', { name: 'V4', exact: true })
    const presetV6 = presetGroup.getByRole('button', { name: 'V6', exact: true })
    const expectedActiveBg = await loadRangeList.getByRole('tab', { name: '实时', exact: true }).evaluate(element => getComputedStyle(element).backgroundColor)
    await presetV4.click()
    await expect(presetV4).toHaveAttribute('aria-pressed', 'true')
    await page.mouse.move(0, 0)
    await expect.poll(() => presetV4.evaluate(element => getComputedStyle(element).backgroundColor)).toBe(expectedActiveBg)
    await presetV4.hover()
    await expect.poll(() => presetV4.evaluate(element => getComputedStyle(element).backgroundColor)).toBe(expectedActiveBg)
    await presetV6.hover()
    await expect(presetV4).toHaveAttribute('aria-pressed', 'true')
    await expect(presetV6).toHaveAttribute('aria-pressed', 'false')
    await presetGroup.getByRole('button', { name: '全部', exact: true }).click()

    // Desktop: mouse hover highlights line series and mouseleave clears immediately.
    await focusV4Btn.hover()
    await expect(trendSection).toHaveAttribute('data-highlighted-task', 'probe-v4')
    await page.mouse.move(0, 0)
    await expect(trendSection).not.toHaveAttribute('data-highlighted-task', /.+/)

    // Desktop: chart tooltip hides cleanly when switching metrics so no stale tooltip lingers.
    const latencyChart = page.locator('[aria-label="独立延迟曲线，纵轴为毫秒，长时间缺报为断点"]')
    await expect(latencyChart).toBeVisible()
    await latencyChart.hover()
    await expect(trendSection.getByText('20 ms', { exact: true })).toBeVisible()
    await metricGroup.getByRole('button', { name: '丢包', exact: true }).click()
    await expect(trendSection.getByText('20 ms', { exact: true })).toBeHidden()
    await metricGroup.getByRole('button', { name: 'RTT', exact: true }).click()

    // Desktop: real keyboard Tab focus triggers :focus-visible highlight and Tab away clears it.
    await page.getByRole('button', { name: '全不选', exact: true }).focus()
    await page.keyboard.press('Tab')
    await expect(focusV4Btn).toBeFocused()
    await expect(trendSection).toHaveAttribute('data-highlighted-task', 'probe-v4')
    await page.keyboard.press('Tab')
    await expect(trendSection).not.toHaveAttribute('data-highlighted-task', /.+/)
  }

  expect(activeProbeRequests).toEqual([])

  // Network quality page keeps default ProbeHistory behavior (no detailInteractions leak).
  await page.goto('/network-quality')
  await expect(page.getByTestId('probe-tab-pill')).toHaveCount(0)
})
