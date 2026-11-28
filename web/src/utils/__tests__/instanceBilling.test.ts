import { describe, expect, it } from 'vitest'
import { REFERENCE_EXCHANGE_RATES } from '../financeHelper'
import { buildInstanceBillingCards } from '../instanceBilling'

const now = Date.parse('2026-10-01T00:00:00Z')
const halfMonthRemaining = '2026-10-16T00:00:00Z'

describe('buildInstanceBillingCards', () => {
  it('keeps original currency across price, monthly average, and remaining value when exchange rate is missing', () => {
    const cards = buildInstanceBillingCards(
      {
        price: 500,
        billing_cycle: 30,
        currency: 'RUB',
        expired_at: halfMonthRemaining,
      },
      { CNY: 1, USD: 0.14799, EUR: 0.1275 },
      'CNY',
      now,
    )

    expect(cards[0]).toMatchObject({
      label: '节点价格',
      value: '₽500.00 RUB',
      unit: ' / 月',
    })
    expect(cards[1]).toMatchObject({
      label: '月均支出',
      value: '₽500.00 RUB',
      unit: ' / 月',
    })
    expect(cards[3]).toMatchObject({
      label: '剩余价值',
      value: '₽250.00',
      unit: 'RUB',
    })
  })

  it('keeps unknown currency codes in original currency without silently treating them as CNY', () => {
    const cards = buildInstanceBillingCards(
      {
        price: 120,
        billing_cycle: 30,
        currency: 'TWD',
        expired_at: halfMonthRemaining,
      },
      REFERENCE_EXCHANGE_RATES,
      'CNY',
      now,
    )

    expect(cards[0]).toMatchObject({
      label: '节点价格',
      value: '120.00 TWD',
      unit: ' / 月',
    })
    expect(cards[1]).toMatchObject({
      label: '月均支出',
      value: '120.00 TWD',
      unit: ' / 月',
    })
    expect(cards[3]).toMatchObject({
      label: '剩余价值',
      value: '60.00',
      unit: 'TWD',
    })
  })

  it('does not require a rate when source and selected currencies match', () => {
    const cards = buildInstanceBillingCards(
      {
        price: 19.99,
        billing_cycle: 30,
        currency: 'CNY',
        expired_at: halfMonthRemaining,
      },
      {},
      'CNY',
      now,
    )

    expect(cards[0]).toMatchObject({
      label: '节点价格',
      value: '¥19.99 CNY',
      unit: ' / 月',
    })
    expect(cards[1]).toMatchObject({
      label: '月均支出',
      value: '¥19.99 CNY',
      unit: ' / 月',
    })
    expect(cards[3]).toMatchObject({
      label: '剩余价值',
      value: '¥10.00',
      unit: 'CNY',
    })
  })

  it('converts normally when a valid exchange rate is present', () => {
    const cards = buildInstanceBillingCards(
      {
        price: 10,
        billing_cycle: 30,
        currency: 'USD',
        expired_at: halfMonthRemaining,
      },
      { CNY: 1, USD: 0.2, EUR: 0.1 },
      'EUR',
      now,
    )

    expect(cards[0]).toMatchObject({
      label: '节点价格',
      value: '€5.00 EUR',
      unit: ' / 月',
    })
    expect(cards[1]).toMatchObject({
      label: '月均支出',
      value: '€5.00 EUR',
      unit: ' / 月',
    })
    expect(cards[3]).toMatchObject({
      label: '剩余价值',
      value: '€2.50',
      unit: 'EUR',
    })
  })

  it('preserves explicit zero-price semantics even when the currency has no exchange rate', () => {
    const cards = buildInstanceBillingCards(
      {
        price: 0,
        billing_cycle: 30,
        currency: 'RUB',
        expired_at: halfMonthRemaining,
      },
      { CNY: 1, USD: 0.14799 },
      'CNY',
      now,
    )

    expect(cards[0]).toMatchObject({
      label: '节点价格',
      value: '¥0.00',
      unit: 'CNY',
    })
    expect(cards[1]).toMatchObject({
      label: '月均支出',
      value: '¥0.00 CNY',
      unit: ' / 月',
    })
    expect(cards[3]).toMatchObject({
      label: '剩余价值',
      value: '¥0.00',
      unit: 'CNY',
    })
  })

  it('keeps non-positive cycle, lifetime, and expired semantics intact in original currency', () => {
    const lifetime = buildInstanceBillingCards(
      {
        price: 500,
        billing_cycle: 0,
        currency: 'RUB',
        expired_at: '9999-01-01T00:00:00Z',
      },
      { CNY: 1 },
      'CNY',
      now,
    )
    expect(lifetime[0]).toMatchObject({
      label: '节点价格',
      value: '₽500.00 RUB',
      unit: ' / 0 天',
    })
    expect(lifetime[1]).toMatchObject({
      label: '月均支出',
      value: '不适用',
      unit: undefined,
    })
    expect(lifetime[3]).toMatchObject({
      label: '剩余价值',
      value: '₽500.00',
      unit: 'RUB',
    })

    const expired = buildInstanceBillingCards(
      {
        price: 500,
        billing_cycle: 30,
        currency: 'RUB',
        expired_at: '2026-09-30T00:00:00Z',
      },
      { CNY: 1 },
      'CNY',
      now,
    )
    expect(expired[3]).toMatchObject({
      label: '剩余价值',
      value: '₽0.00',
      unit: 'RUB',
    })
  })
})
