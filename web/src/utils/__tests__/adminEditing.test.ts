import type { ManagedNode, NotificationChannel } from '../admin'
import { describe, expect, it } from 'vitest'
import {
  buildAssignableNodes,
  buildSelectableChannels,
  normalizePublicRemark,
  pruneMissingChannels,
  requiresGlobalScopeConfirmation,
  shouldGuardGlobalScope,
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
})
