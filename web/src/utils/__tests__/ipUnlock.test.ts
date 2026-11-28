import type { IpUnlockReport } from '@/utils/rpc'
import { describe, expect, it } from 'vitest'
import { getGoogleUnlockDisplay, getOrderedUnlockServices, getUnlockPanelState } from '../ipUnlock'

function makeReport(overrides: Partial<IpUnlockReport> = {}): IpUnlockReport {
  return {
    family: 4,
    checked_at_unix_ms: 1_800_000_000_000,
    address_changed: false,
    google: {
      status: 'normal',
      region: 'US',
    },
    services: [
      { id: 'youtube', status: 'unlocked', region: 'US' },
      { id: 'netflix', status: 'unlocked', region: 'US' },
      { id: 'disney_plus', status: 'unlocked', region: 'US' },
      { id: 'chatgpt', status: 'unlocked', region: 'US' },
      { id: 'reddit', status: 'unlocked', region: 'US' },
      { id: 'tiktok', status: 'unlocked', region: 'US' },
    ],
    ...overrides,
  }
}

describe('ipUnlock helpers', () => {
  it('omits TikTok on IPv6 and keeps all six services on IPv4', () => {
    const v4 = getOrderedUnlockServices(makeReport({ family: 4 }), 4)
    expect(v4.map(item => item.id)).toEqual([
      'youtube',
      'netflix',
      'disney_plus',
      'chatgpt',
      'reddit',
      'tiktok',
    ])

    const v6 = getOrderedUnlockServices(makeReport({ family: 6 }), 6)
    expect(v6.map(item => item.id)).toEqual([
      'youtube',
      'netflix',
      'disney_plus',
      'chatgpt',
      'reddit',
    ])

    const emptyV6 = getOrderedUnlockServices(undefined, 6)
    expect(emptyV6.map(item => item.id)).toEqual([
      'youtube',
      'netflix',
      'disney_plus',
      'chatgpt',
      'reddit',
    ])
  })

  it('omits standalone hint for normal Google localization while keeping warning hints', () => {
    const usNormal = getGoogleUnlockDisplay(makeReport())
    expect(usNormal.label).toBe('未送中 (US)')
    expect(usNormal.hint).toBe('')

    const hkNormal = getGoogleUnlockDisplay(makeReport({
      google: { status: 'normal', region: 'HK' },
    }))
    expect(hkNormal.label).toBe('未送中 (HK)')
    expect(hkNormal.hint).toBe('')

    const china = getGoogleUnlockDisplay(makeReport({
      google: { status: 'china', region: 'CN' },
    }))
    expect(china.label).toBe('已送中 (CN)')
    expect(china.hint).toBe('Google / YouTube 识别为中国大陆')
    expect(china.isChina).toBe(true)
  })

  it('distinguishes missing unlock records from address_changed in the detail panel state', () => {
    const ready = getUnlockPanelState(makeReport({ family: 4, address_changed: false }), 4)
    expect(ready.status).toBe('ready')

    const missing = getUnlockPanelState(undefined, 6)
    expect(missing.status).toBe('empty')
    expect(missing.badgeLabel).toBe('暂无记录')
    expect(missing.title).toContain('IPv6')
    expect(missing.title).toContain('暂无')
    expect(missing.description).not.toContain('回程')
    expect(missing.description).not.toContain('点击')

    const changed = getUnlockPanelState(makeReport({ family: 6, address_changed: true }), 6)
    expect(changed.status).toBe('address_changed')
    expect(changed.badgeLabel).toBe('地址已变更')
    expect(changed.title).toContain('IPv6')
    expect(changed.title).toContain('不再适用于当前地址')
    expect(changed.description).not.toContain('回程')
    expect(changed.title).not.toBe(missing.title)
    expect(changed.badgeLabel).not.toBe(missing.badgeLabel)
  })
})
