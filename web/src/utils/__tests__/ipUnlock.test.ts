import type { IpUnlockReport } from '@/utils/rpc'
import { describe, expect, it } from 'vitest'
import { getGoogleUnlockDisplay, getOrderedUnlockServices } from '../ipUnlock'

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
})
