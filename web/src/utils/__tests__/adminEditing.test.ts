import type { AlertRule, ManagedNode, NotificationChannel, ProbeTask } from '../admin'
import { describe, expect, it } from 'vitest'
import {
  buildAssignableNodes,
  buildSelectableChannels,
  cloneAlertRule,
  cloneNotificationChannel,
  cloneProbeTask,
  createClientUuid,
  isAlertRuleDraftDirty,
  isChannelDraftDirty,
  isProbeDraftDirty,
  normalizePublicRemark,
  pruneMissingChannels,
  requiresGlobalScopeConfirmation,
  shouldGuardGlobalScope,
  summarizeAlertChannels,
  summarizeNodeTargetScope,
  validateAlertDraftChannels,
  validatePublicRemark,
  validateScopedNodeSelection,
} from '../adminEditing'

function makeNode(id: string, name: string, disabled = false): ManagedNode {
  return {
    id,
    name,
    region: 'SG',
    group: '',
    weight: 0,
    hidden: false,
    tags: '',
    public_remark: '',
    price: 0,
    currency: 'USD',
    billing_cycle_days: 30,
    expired_at_unix_ms: null,
    auto_renewal: false,
    traffic_limit_bytes: 0,
    traffic_limit_type: 'sum',
    traffic_reset_day: 1,
    disabled,
  }
}

function makeChannel(id: string, name: string, enabled = true): NotificationChannel {
  return {
    id,
    name,
    kind: 'webhook',
    url: 'https://example.invalid/hook',
    enabled,
  }
}

describe('admin editing helpers', () => {
  it('surfaces revoked and missing assigned nodes while keeping active node labels intact', () => {
    const nodes = [
      makeNode('active-1', 'Alpha'),
      makeNode('revoked-1', 'Beta', true),
      makeNode('revoked-unused', 'Gamma', true),
    ]
    const options = buildAssignableNodes(nodes, ['revoked-1', 'deleted-1'], ['revoked-1'])
    expect(options).toEqual([
      {
        id: 'active-1',
        label: 'Alpha',
        revoked: false,
        missing: false,
      },
      {
        id: 'revoked-1',
        label: 'Beta（凭据已撤销）',
        revoked: true,
        missing: false,
      },
      {
        id: 'deleted-1',
        label: 'deleted-1（节点已不存在）',
        revoked: false,
        missing: true,
      },
    ])
  })

  it('blocks saving revoked nodes and prevents silent widening across save-disabled and reopen flows', () => {
    const nodes = [
      makeNode('active-1', 'Alpha'),
      makeNode('revoked-1', 'Beta', true),
    ]

    const explicitGuard = shouldGuardGlobalScope({
      id: 'item-1',
      enabled: true,
      node_ids: ['revoked-1'],
    })
    expect(explicitGuard).toBe(true)

    expect(validateScopedNodeSelection({
      guardGlobalScope: explicitGuard,
      selectedIds: ['revoked-1'],
      enabled: true,
      confirmGlobalScope: false,
      nodes,
    })).toContain('已撤销或不存在')

    expect(requiresGlobalScopeConfirmation(explicitGuard, [], true)).toBe(true)
    expect(validateScopedNodeSelection({
      guardGlobalScope: explicitGuard,
      selectedIds: [],
      enabled: true,
      confirmGlobalScope: false,
      nodes,
    })).toContain('静默扩大到所有活动节点')

    // Disabling the item allows saving with an empty target list.
    expect(validateScopedNodeSelection({
      guardGlobalScope: explicitGuard,
      selectedIds: [],
      enabled: false,
      confirmGlobalScope: false,
      nodes,
    })).toBeNull()

    // Reopening that disabled empty-target item (or a backend-disabled deleted-node
    // record) and re-enabling it without targets still requires explicit confirmation.
    const reopenedDisabledGuard = shouldGuardGlobalScope({
      id: 'item-1',
      enabled: false,
      node_ids: [],
    })
    expect(reopenedDisabledGuard).toBe(true)
    expect(validateScopedNodeSelection({
      guardGlobalScope: reopenedDisabledGuard,
      selectedIds: [],
      enabled: true,
      confirmGlobalScope: false,
      nodes,
    })).toContain('静默扩大到所有活动节点')
    expect(validateScopedNodeSelection({
      guardGlobalScope: reopenedDisabledGuard,
      selectedIds: [],
      enabled: true,
      confirmGlobalScope: true,
      nodes,
    })).toBeNull()
    expect(validateScopedNodeSelection({
      guardGlobalScope: reopenedDisabledGuard,
      selectedIds: ['active-1'],
      enabled: true,
      confirmGlobalScope: false,
      nodes,
    })).toBeNull()

    // Already-enabled global items and brand-new items remain frictionless to edit.
    const enabledGlobalGuard = shouldGuardGlobalScope({
      id: 'global-1',
      enabled: true,
      node_ids: [],
    })
    expect(enabledGlobalGuard).toBe(false)
    expect(validateScopedNodeSelection({
      guardGlobalScope: enabledGlobalGuard,
      selectedIds: [],
      enabled: true,
      confirmGlobalScope: false,
      nodes,
    })).toBeNull()

    const newItemGuard = shouldGuardGlobalScope({
      id: '',
      enabled: true,
      node_ids: [],
    })
    expect(newItemGuard).toBe(false)
    expect(validateScopedNodeSelection({
      guardGlobalScope: newItemGuard,
      selectedIds: [],
      enabled: true,
      confirmGlobalScope: false,
      nodes,
    })).toBeNull()
  })

  it('surfaces, validates, and prunes missing channels from stale alert drafts', () => {
    const channels = [
      makeChannel('ch-1', 'Primary', true),
      makeChannel('ch-2', 'Backup', false),
    ]
    expect(buildSelectableChannels(channels, ['ch-1', 'stale-ch'])).toEqual([
      {
        id: 'ch-1',
        label: 'Primary',
        missing: false,
      },
      {
        id: 'ch-2',
        label: 'Backup（已停用）',
        missing: false,
      },
      {
        id: 'stale-ch',
        label: 'stale-ch（渠道已失效）',
        missing: true,
      },
    ])
    expect(validateAlertDraftChannels(['ch-1', 'stale-ch'], channels)).toContain('已失效的通知渠道')
    expect(pruneMissingChannels(['ch-1', 'stale-ch', 'ch-2'], channels)).toEqual(['ch-1', 'ch-2'])
    expect(validateAlertDraftChannels(['ch-1'], channels)).toBeNull()
  })

  it('aligns multiline public remark normalization and control-character boundaries', () => {
    expect(normalizePublicRemark('first\r\nsecond\nthird')).toBe('first\nsecond\nthird')
    expect(validatePublicRemark('first\r\nsecond\nthird')).toBeNull()
    expect(validatePublicRemark('bare\rreturn')).not.toBeNull()
    expect(validatePublicRemark('has\ttab')).not.toBeNull()
    expect(validatePublicRemark('has\u0000nul')).not.toBeNull()
    expect(validatePublicRemark('c1\u0085nel')).not.toBeNull()
    expect(validatePublicRemark('a'.repeat(2049))).not.toBeNull()
  })

  it('summarizes target node scopes and channel assignments without leaking webhook URLs', () => {
    const nodes = [
      makeNode('active-1', 'Alpha'),
      makeNode('revoked-1', 'Beta', true),
    ]
    expect(summarizeNodeTargetScope([], nodes)).toEqual({
      mode: 'all',
      label: '全部活动节点',
      totalCount: 0,
      activeCount: 0,
      revokedCount: 0,
      missingCount: 0,
      warningLabel: '',
    })
    expect(summarizeNodeTargetScope(['active-1', 'revoked-1', 'active-1'], nodes)).toEqual({
      mode: 'scoped',
      label: '指定 2 个节点',
      totalCount: 2,
      activeCount: 1,
      revokedCount: 1,
      missingCount: 0,
      warningLabel: '含 1 个已撤销节点',
    })
    expect(summarizeNodeTargetScope(['revoked-1', 'missing-1'], nodes)).toEqual({
      mode: 'scoped',
      label: '指定 2 个节点',
      totalCount: 2,
      activeCount: 0,
      revokedCount: 1,
      missingCount: 1,
      warningLabel: '含 1 个已撤销、1 个已不存在节点',
    })

    const channels = [
      {
        ...makeChannel('ch-1', 'Primary', true),
        url: 'https://example.invalid/top-secret-token',
      },
      makeChannel('ch-2', 'Backup', false),
    ]
    expect(summarizeAlertChannels([], channels)).toEqual({
      mode: 'in-app-only',
      label: '仅站内记录',
      totalCount: 0,
      enabledCount: 0,
      disabledCount: 0,
      missingCount: 0,
      warningLabel: '',
    })
    const singleSummary = summarizeAlertChannels(['ch-1'], channels)
    expect(singleSummary.label).toBe('通知渠道：Primary')
    expect(singleSummary.warningLabel).toBe('')
    expect(JSON.stringify(singleSummary)).not.toContain('top-secret-token')

    const mixedSummary = summarizeAlertChannels(['ch-1', 'ch-2', 'stale-ch'], channels)
    expect(mixedSummary).toEqual({
      mode: 'channels',
      label: '通知渠道：Primary 等 3 个',
      totalCount: 3,
      enabledCount: 1,
      disabledCount: 1,
      missingCount: 1,
      warningLabel: '含 1 个已失效、1 个已停用渠道',
    })
    expect(summarizeAlertChannels(['ch-2'], channels)).toEqual({
      mode: 'in-app-only',
      label: '仅站内记录（无启用外发渠道）',
      totalCount: 1,
      enabledCount: 0,
      disabledCount: 1,
      missingCount: 0,
      warningLabel: '含 1 个已停用渠道',
    })
    expect(summarizeAlertChannels(['stale-ch'], channels)).toEqual({
      mode: 'in-app-only',
      label: '仅站内记录（无启用外发渠道）',
      totalCount: 1,
      enabledCount: 0,
      disabledCount: 0,
      missingCount: 1,
      warningLabel: '含 1 个已失效渠道',
    })
    expect(summarizeAlertChannels(['ch-2', 'stale-ch'], channels)).toEqual({
      mode: 'in-app-only',
      label: '仅站内记录（无启用外发渠道）',
      totalCount: 2,
      enabledCount: 0,
      disabledCount: 1,
      missingCount: 1,
      warningLabel: '含 1 个已失效、1 个已停用渠道',
    })

    const uuid1 = createClientUuid()
    const uuid2 = createClientUuid()
    expect(uuid1).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    expect(uuid2).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    expect(uuid1).not.toBe(uuid2)
  })

  it('tracks dirty state accurately for probe, channel, and alert rule drafts', () => {
    const probe: ProbeTask = {
      id: 'probe-1',
      name: 'Edge Ping',
      weight: 10,
      kind: 'icmp',
      target: 'example.com',
      interval_seconds: 60,
      timeout_seconds: 5,
      enabled: true,
      node_ids: ['n1', 'n2'],
    }
    const probeCopy = cloneProbeTask(probe)
    expect(isProbeDraftDirty({
      ...probeCopy,
      interval_seconds: '60' as unknown as number,
      node_ids: ['n2', 'n1'],
    }, probe)).toBe(false)
    expect(isProbeDraftDirty({
      ...probeCopy,
      target: 'edge.example.com',
    }, probe)).toBe(true)
    expect(isProbeDraftDirty(probeCopy, probe, true)).toBe(true)

    const channel = makeChannel('ch-1', 'Primary', false)
    const channelCopy = cloneNotificationChannel(channel)
    expect(isChannelDraftDirty(channelCopy, channel)).toBe(false)
    expect(isChannelDraftDirty({
      ...channelCopy,
      enabled: true,
    }, channel)).toBe(true)

    const rule: AlertRule = {
      id: 'rule-1',
      name: 'High CPU',
      metric: 'cpu',
      threshold: 85,
      duration_seconds: 120,
      cooldown_seconds: 900,
      node_ids: ['n1'],
      channel_ids: ['ch-1', 'ch-2'],
      enabled: true,
    }
    const ruleCopy = cloneAlertRule(rule)
    expect(isAlertRuleDraftDirty({
      ...ruleCopy,
      threshold: '85.0' as unknown as number,
      channel_ids: ['ch-2', 'ch-1'],
    }, rule)).toBe(false)
    expect(isAlertRuleDraftDirty({
      ...ruleCopy,
      channel_ids: ['ch-1'],
    }, rule)).toBe(true)
    expect(isAlertRuleDraftDirty(ruleCopy, rule, true)).toBe(true)
  })
})
