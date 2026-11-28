import { describe, expect, it } from 'vitest'
import {
  billedTraffic,
  formatApproxTrafficAmount,
  formatIntegerBytes,
  formatTrafficAmount,
  parseTrafficAmount,
} from '../trafficCorrection'

describe('local traffic correction amounts', () => {
  it('distinguishes decimal GB from binary GiB and rounds only sub-byte fractions', () => {
    expect(parseTrafficAmount('1', 'gb')).toBe('1000000000')
    expect(parseTrafficAmount('1', 'gib')).toBe('1073741824')
    expect(parseTrafficAmount('0.0000000005', 'gb')).toBe('1')
    expect(parseTrafficAmount('0.1', 'gib')).toBe('107374182')
  })

  it.each([
    '0',
    '1',
    '9007199254740993',
    '9223372036854775807',
  ])('round-trips %s bytes without Number precision loss', (value) => {
    expect(parseTrafficAmount(formatTrafficAmount(value, 'gb'), 'gb')).toBe(value)
    expect(parseTrafficAmount(formatTrafficAmount(value, 'gib'), 'gib')).toBe(value)
  })

  it.each([
    '',
    '-1',
    'NaN',
    'Infinity',
    '1e3',
    '1,000',
    '0x10',
    '99999999999999999999',
  ])('rejects invalid or overflowing input %s', (value) => {
    expect(() => parseTrafficAmount(value, 'gb')).toThrow()
  })

  it('previews each billing direction without assigning a total to both counters', () => {
    expect(billedTraffic('20', '30', 'sum')).toBe('50')
    expect(billedTraffic('20', '30', 'max')).toBe('30')
    expect(billedTraffic('20', '30', 'min')).toBe('20')
    expect(billedTraffic('20', '30', 'up')).toBe('20')
    expect(billedTraffic('20', '30', 'down')).toBe('30')
  })

  it('formats approximate summaries and integer bytes with pure BigInt rounding across i64 boundaries', () => {
    expect(formatApproxTrafficAmount('0', 'gib')).toEqual({ text: '0', approximate: false })
    expect(formatApproxTrafficAmount('1000000000', 'gb')).toEqual({ text: '1', approximate: false })
    expect(formatApproxTrafficAmount('429496729600', 'gib')).toEqual({ text: '400', approximate: false })
    expect(formatApproxTrafficAmount('1250000000', 'gb')).toEqual({ text: '1.25', approximate: false })

    // Synthetic exact byte set: up + down === total in both exact bytes and 2-decimal approx GiB.
    expect(formatApproxTrafficAmount('131642089473', 'gib')).toEqual({ text: '约 122.60', approximate: true })
    expect(formatApproxTrafficAmount('198234685442', 'gib')).toEqual({ text: '约 184.62', approximate: true })
    expect(formatApproxTrafficAmount('329876774915', 'gib')).toEqual({ text: '约 307.22', approximate: true })
    expect(formatApproxTrafficAmount('1', 'gib')).toEqual({ text: '约 0.00', approximate: true })
    expect(formatApproxTrafficAmount('1000000000', 'gib')).toEqual({ text: '约 0.93', approximate: true })
    expect(formatApproxTrafficAmount('1073741824', 'gb')).toEqual({ text: '约 1.07', approximate: true })

    // Half-up rounding boundary (0.005 GB -> 0.01 GB; 0.004999999 GB -> 0.00 GB).
    expect(formatApproxTrafficAmount('5000000', 'gb')).toEqual({ text: '约 0.01', approximate: true })
    expect(formatApproxTrafficAmount('4999999', 'gb')).toEqual({ text: '约 0.00', approximate: true })

    // Above 2^53 and i64::MAX without Number conversion loss.
    expect(formatApproxTrafficAmount('9007199254740993', 'gb')).toEqual({ text: '约 9007199.25', approximate: true })
    expect(formatApproxTrafficAmount('9223372036854775807', 'gib')).toEqual({ text: '约 8589934592.00', approximate: true })
    expect(formatApproxTrafficAmount('9223372036854775807', 'gb')).toEqual({ text: '约 9223372036.85', approximate: true })

    expect(formatIntegerBytes('0')).toBe('0 B')
    expect(formatIntegerBytes('131642089473')).toBe('131,642,089,473 B')
    expect(formatIntegerBytes('9223372036854775807')).toBe('9,223,372,036,854,775,807 B')
  })
})
