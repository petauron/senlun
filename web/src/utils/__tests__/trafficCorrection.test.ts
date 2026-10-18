import { describe, expect, it } from 'vitest'
import { billedTraffic, formatTrafficAmount, parseTrafficAmount } from '../trafficCorrection'

describe('local traffic correction amounts', () => {
  it('distinguishes decimal GB from binary GiB and rounds only sub-byte fractions', () => {
    expect(parseTrafficAmount('1', 'gb')).toBe('1000000000')
    expect(parseTrafficAmount('1', 'gib')).toBe('1073741824')
    expect(parseTrafficAmount('0.0000000005', 'gb')).toBe('1')
    expect(parseTrafficAmount('0.1', 'gib')).toBe('107374182')
  })

  it.each(['0', '1', '9007199254740993', '9223372036854775807'])('round-trips %s bytes without Number precision loss', (value) => {
    expect(parseTrafficAmount(formatTrafficAmount(value, 'gb'), 'gb')).toBe(value)
    expect(parseTrafficAmount(formatTrafficAmount(value, 'gib'), 'gib')).toBe(value)
  })

  it.each(['', '-1', 'NaN', 'Infinity', '1e3', '1,000', '0x10', '99999999999999999999'])('rejects invalid or overflowing input %s', (value) => {
    expect(() => parseTrafficAmount(value, 'gb')).toThrow()
  })

  it('previews each billing direction without assigning a total to both counters', () => {
    expect(billedTraffic('20', '30', 'sum')).toBe('50')
    expect(billedTraffic('20', '30', 'max')).toBe('30')
    expect(billedTraffic('20', '30', 'min')).toBe('20')
    expect(billedTraffic('20', '30', 'up')).toBe('20')
    expect(billedTraffic('20', '30', 'down')).toBe('30')
  })
})
