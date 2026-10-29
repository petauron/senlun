import type { ManagedNode } from '../admin'
import { describe, expect, it } from 'vitest'
import { nodeMetadata } from '../admin'

describe('node metadata updates', () => {
  it('excludes read-only counters from the strict write contract', () => {
    const metadata: ManagedNode = {
      id: 'node-1',
      name: 'Node',
      region: 'SG',
      group: '',
      weight: 0,
      hidden: false,
      tags: '',
      public_remark: '',
      ipv4: '1.1.1.1',
      ipv6: '',
      price: 2.5,
      currency: 'USD',
      billing_cycle_days: 30,
      expired_at_unix_ms: null,
      auto_renewal: false,
      traffic_source: 'local',
      traffic_limit_bytes: 1024,
      traffic_limit_type: 'sum',
      traffic_reset_day: 1,
    }
    const stateNode = {
      ...metadata,
      traffic_used_up: 42,
      traffic_used_down: 100,
      disabled_at_ms: null,
      traffic_next_reset_unix_ms: 1_800_000_000_000,
    }
    expect(nodeMetadata(stateNode)).toEqual(metadata)
    expect(nodeMetadata(stateNode)).not.toHaveProperty('traffic_used_up')
    expect(nodeMetadata({ ...stateNode, traffic_reset_day: 29 }).traffic_reset_day).toBe(29)
  })
})
