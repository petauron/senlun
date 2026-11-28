import type { ManagedNode } from '../admin'
import { describe, expect, it } from 'vitest'
import {
  filterManagedNodes,
  NODE_DANGER_ACTION_ORDER,
  NODE_DANGER_ACTIONS,
  NODE_LIST_FILTER_OPTIONS,
  resolveNodeSelection,
} from '../adminNodes'

function makeNode(overrides: Partial<ManagedNode> = {}): ManagedNode {
  return {
    id: '11111111-aaaa-bbbb-cccc-000000000001',
    name: 'Alpha SG',
    region: 'SG',
    group: 'Asia',
    weight: 0,
    hidden: false,
    disabled: false,
    tags: '',
    public_remark: 'Primary edge node',
    ipv4: '203.0.113.10',
    ipv6: '',
    price: 10,
    currency: 'USD',
    billing_cycle_days: 30,
    expired_at_unix_ms: null,
    auto_renewal: false,
    traffic_source: 'local',
    traffic_limit_bytes: 0,
    traffic_limit_type: 'sum',
    traffic_reset_day: 1,
    ...overrides,
  }
}

describe('adminNodes search, filter, selection, and danger action helpers', () => {
  const nodes = [
    makeNode({
      id: 'aa112233-0000-0000-0000-000000000001',
      name: 'Alpha SG',
      region: 'SG',
      group: 'Asia',
      public_remark: 'Primary Singapore edge',
    }),
    makeNode({
      id: 'bb445566-0000-0000-0000-000000000002',
      name: 'Beta JP',
      region: 'JP',
      group: 'Asia',
      hidden: true,
      public_remark: 'Hidden staging box',
    }),
    makeNode({
      id: 'cc778899-0000-0000-0000-000000000003',
      name: 'Gamma US',
      region: 'US',
      group: 'Americas',
      disabled: true,
      traffic_source: 'provider',
      public_remark: 'Provider metered relay',
    }),
  ]

  it('filters strictly on real ManagedNode fields without synthetic online status', () => {
    expect(NODE_LIST_FILTER_OPTIONS.map(opt => opt.value)).toEqual([
      'all',
      'hidden',
      'disabled',
      'provider',
    ])
    expect(filterManagedNodes(nodes, '', 'all')).toHaveLength(3)
    expect(filterManagedNodes(nodes, '', 'hidden').map(n => n.id)).toEqual([nodes[1]!.id])
    expect(filterManagedNodes(nodes, '', 'disabled').map(n => n.id)).toEqual([nodes[2]!.id])
    expect(filterManagedNodes(nodes, '', 'provider').map(n => n.id)).toEqual([nodes[2]!.id])
  })

  it('searches across name, group, region, public_remark, and id prefix case-insensitively', () => {
    expect(filterManagedNodes(nodes, 'beta', 'all').map(n => n.id)).toEqual([nodes[1]!.id])
    expect(filterManagedNodes(nodes, 'americas', 'all').map(n => n.id)).toEqual([nodes[2]!.id])
    expect(filterManagedNodes(nodes, 'sg', 'all').map(n => n.id)).toEqual([nodes[0]!.id])
    expect(filterManagedNodes(nodes, 'staging', 'all').map(n => n.id)).toEqual([nodes[1]!.id])
    expect(filterManagedNodes(nodes, 'cc7788', 'all').map(n => n.id)).toEqual([nodes[2]!.id])
    // Non-prefix ID substrings should not accidentally match unrelated nodes.
    expect(filterManagedNodes(nodes, '445566', 'all')).toEqual([])
  })

  it('resolves desktop default, mobile list state, valid deep links, and explicit missing deep links without silent fallback', () => {
    expect(resolveNodeSelection({
      nodes,
      queryNodeId: null,
      isDesktop: true,
    })).toEqual({
      mode: 'selected',
      node: nodes[0],
    })

    expect(resolveNodeSelection({
      nodes,
      queryNodeId: null,
      isDesktop: false,
    })).toEqual({
      mode: 'list',
    })

    expect(resolveNodeSelection({
      nodes,
      queryNodeId: nodes[1]!.id,
      isDesktop: false,
    })).toEqual({
      mode: 'selected',
      node: nodes[1],
    })

    expect(resolveNodeSelection({
      nodes,
      queryNodeId: 'missing-node-id',
      isDesktop: true,
    })).toEqual({
      mode: 'missing',
      requestedId: 'missing-node-id',
    })

    expect(resolveNodeSelection({
      nodes,
      queryNodeId: 'missing-node-id',
      isDesktop: false,
    })).toEqual({
      mode: 'missing',
      requestedId: 'missing-node-id',
    })

    expect(resolveNodeSelection({
      nodes: [],
      queryNodeId: null,
      isDesktop: true,
    })).toEqual({
      mode: 'empty',
    })

    expect(resolveNodeSelection({
      nodes: [],
      queryNodeId: 'missing-when-empty',
      isDesktop: true,
    })).toEqual({
      mode: 'missing',
      requestedId: 'missing-when-empty',
    })

    // Pinned node survives viewport narrowing and list reordering without silently switching.
    expect(resolveNodeSelection({
      nodes: [nodes[1]!, nodes[0]!, nodes[2]!],
      queryNodeId: null,
      pinnedNodeId: nodes[0]!.id,
      isDesktop: false,
    })).toEqual({
      mode: 'selected',
      node: nodes[0],
    })

    // A previously pinned node that disappears from refreshed nodes must enter explicit removed state rather than silently switching to another node.
    expect(resolveNodeSelection({
      nodes: [nodes[1]!, nodes[2]!],
      queryNodeId: null,
      pinnedNodeId: nodes[0]!.id,
      isDesktop: true,
    })).toEqual({
      mode: 'removed',
      removedId: nodes[0]!.id,
    })
    expect(resolveNodeSelection({
      nodes: [nodes[1]!, nodes[2]!],
      queryNodeId: null,
      pinnedNodeId: nodes[0]!.id,
      isDesktop: false,
    })).toEqual({
      mode: 'removed',
      removedId: nodes[0]!.id,
    })
  })

  it('provides explicit target node names, concrete confirm verbs, and accurate revoke re-enrollment copy', () => {
    expect(NODE_DANGER_ACTION_ORDER).toEqual([
      'rotate',
      'revoke',
      'reset-traffic',
      'delete',
    ])
    for (const action of NODE_DANGER_ACTION_ORDER) {
      const meta = NODE_DANGER_ACTIONS[action]
      expect(meta.confirmLabel).not.toBe('确认执行')
      expect(meta.confirmLabel).toMatch(/^确认/)
      expect(meta.description('Alpha SG')).toContain('Alpha SG')
    }
    expect(NODE_DANGER_ACTIONS.revoke.description('Alpha SG')).toContain('独立新节点')
    expect(NODE_DANGER_ACTIONS.revoke.description('Alpha SG')).toContain('不会因重新注册恢复')
  })
})
