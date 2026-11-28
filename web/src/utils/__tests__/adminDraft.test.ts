import type { ManagedNode, SiteSettings } from '../admin'
import { describe, expect, it } from 'vitest'
import {
  createNodeDraftSnapshot,
  isNodeDraftDirty,
  isSiteSettingsDirty,
  normalizeSiteSettings,
  shouldSyncNodeDraftFromServer,
} from '../adminDraft'
import { decideAdminDeparture, shouldWarnBeforeUnload } from '../authFlow'

function nodeFixture(overrides: Partial<ManagedNode> = {}): ManagedNode {
  return {
    id: 'node-1',
    name: 'Node One',
    region: 'SG',
    group: 'Asia',
    weight: 0,
    hidden: false,
    tags: 'SG<blue>',
    public_remark: 'Line 1\nLine 2',
    ipv4: '203.0.113.10',
    ipv6: '',
    price: 10,
    currency: 'USD',
    billing_cycle_days: 30,
    expired_at_unix_ms: null,
    auto_renewal: false,
    traffic_source: 'local',
    traffic_limit_bytes: 1024 ** 3,
    traffic_limit_type: 'sum',
    traffic_reset_day: 1,
    peer_probe: {
      kind: 'tcp',
      family: 'auto',
      disabled: false,
    },
    ...overrides,
  }
}

describe('admin draft comparison and server sync helpers', () => {
  it('treats freshly loaded and numerically equivalent node fields as clean', () => {
    const snapshot = createNodeDraftSnapshot(nodeFixture())
    expect(isNodeDraftDirty({
      draft: {
        ...snapshot.draft,
        weight: '0' as unknown as number,
        price: '10.0' as unknown as number,
      },
      expires: snapshot.expires,
      quotaGiB: '1',
      newTagText: '   ',
    }, snapshot)).toBe(false)
  })

  it('detects dirty node remarks, cleared numeric fields, and unadded tag text', () => {
    const snapshot = createNodeDraftSnapshot(nodeFixture())
    expect(isNodeDraftDirty({
      draft: {
        ...snapshot.draft,
        public_remark: 'Edited remark',
      },
      expires: snapshot.expires,
      quotaGiB: snapshot.quotaGiB,
      newTagText: '',
    }, snapshot)).toBe(true)

    expect(isNodeDraftDirty({
      draft: snapshot.draft,
      expires: snapshot.expires,
      quotaGiB: '',
      newTagText: '',
    }, snapshot)).toBe(true)

    expect(isNodeDraftDirty({
      draft: snapshot.draft,
      expires: snapshot.expires,
      quotaGiB: snapshot.quotaGiB,
      newTagText: '新标签',
    }, snapshot)).toBe(true)
  })

  it('preserves dirty drafts on background refreshes of the same node but syncs on switch or explicit reset', () => {
    expect(shouldSyncNodeDraftFromServer({
      incomingNodeId: 'node-1',
      loadedNodeId: 'node-1',
      hasUnsavedChanges: true,
      forceReset: false,
    })).toBe(false)

    expect(shouldSyncNodeDraftFromServer({
      incomingNodeId: 'node-1',
      loadedNodeId: 'node-1',
      hasUnsavedChanges: false,
      forceReset: false,
    })).toBe(true)

    expect(shouldSyncNodeDraftFromServer({
      incomingNodeId: 'node-1',
      loadedNodeId: 'node-1',
      hasUnsavedChanges: true,
      forceReset: true,
    })).toBe(true)

    expect(shouldSyncNodeDraftFromServer({
      incomingNodeId: 'node-2',
      loadedNodeId: 'node-1',
      hasUnsavedChanges: true,
      forceReset: false,
    })).toBe(true)
  })

  it('normalizes legacy earth_view_mode and optional booleans for site settings dirty checks', () => {
    const raw: SiteSettings = {
      site_name: 'Senlun',
      private_site: true,
      agent_interval_seconds: 3,
      peer_probe_interval_seconds: 60,
      earth_view_mode: 'earth',
    }
    const baseline = normalizeSiteSettings(raw)
    expect(baseline.earth_view_mode).toBe('maps')
    expect(isSiteSettingsDirty({ ...baseline }, baseline)).toBe(false)
    expect(isSiteSettingsDirty({
      ...baseline,
      site_name: 'Senlun Updated',
    }, baseline)).toBe(true)
  })

  it('gates departure and beforeunload strictly on active login and dirty state', () => {
    expect(decideAdminDeparture({
      loggedIn: true,
      stayingInSameContext: false,
      hasUnsavedChanges: true,
      busy: false,
    })).toBe('confirm')
    expect(decideAdminDeparture({
      loggedIn: true,
      stayingInSameContext: false,
      hasUnsavedChanges: true,
      busy: true,
    })).toBe('block')
    expect(decideAdminDeparture({
      loggedIn: false,
      stayingInSameContext: false,
      hasUnsavedChanges: true,
      busy: true,
    })).toBe('allow')
    expect(shouldWarnBeforeUnload(true, true)).toBe(true)
    expect(shouldWarnBeforeUnload(true, false)).toBe(false)
    expect(shouldWarnBeforeUnload(false, true)).toBe(false)
  })
})
