import type { NodeData } from '@/stores/nodes'
import type { CurrencyCode } from '@/utils/financeHelper'
import { CURRENCY_SYMBOLS, resolveCurrencyCode } from '@/utils/financeHelper'
import { getBillingCycleText, getExpireText, getExpireTextClass } from '@/utils/tagHelper'

export interface InstanceBillingCard {
  label: string
  value: string
  unit?: string
  icon: string
  valueClass?: string
}

export type InstanceBillingNode = Pick<NodeData, 'price' | 'billing_cycle' | 'currency' | 'expired_at'>

const EXPIRES_IN_DAYS = /^(\d+)\s+天$/

function validRate(
  rates: Partial<Record<CurrencyCode, number>>,
  code: CurrencyCode | null,
): number | null {
  if (!code)
    return null
  const rate = rates[code]
  return typeof rate === 'number' && Number.isFinite(rate) && rate > 0
    ? rate
    : null
}

function formatAmount(amount: number, symbol: string): string {
  const compact = Math.abs(amount) >= 100000
  const value = new Intl.NumberFormat('zh-CN', {
    maximumFractionDigits: 2,
    minimumFractionDigits: compact ? 0 : 2,
    notation: compact ? 'compact' : 'standard',
  }).format(amount)
  return `${symbol}${value}`
}

export function buildInstanceBillingCards(
  node: InstanceBillingNode,
  rates: Partial<Record<CurrencyCode, number>>,
  selectedCurrency: CurrencyCode,
  nowMs: number,
): InstanceBillingCard[] {
  const selectedRate = validRate(rates, selectedCurrency)
  const displayCurrency: CurrencyCode = selectedRate !== null
    ? selectedCurrency
    : 'CNY'
  const displayRate = selectedRate ?? validRate(rates, 'CNY') ?? 1

  const sourceCode = resolveCurrencyCode(node.currency)
  const rawCurrency = String(node.currency || '').trim().toUpperCase()
  const sourceLabel = sourceCode ?? (rawCurrency || 'CNY')
  const sourceSymbol = sourceCode ? CURRENCY_SYMBOLS[sourceCode] : ''
  const sourceRate = validRate(rates, sourceCode)

  const hasPrice = Number.isFinite(node.price) && node.price > 0
  const hasCycle = Number.isFinite(node.billing_cycle) && node.billing_cycle > 0

  let cardCurrency: string = displayCurrency
  let cardSymbol: string = CURRENCY_SYMBOLS[displayCurrency]
  let cardPrice = 0

  if (hasPrice) {
    if (sourceCode === displayCurrency) {
      cardPrice = node.price
    }
    else if (sourceRate !== null) {
      cardPrice = node.price / sourceRate * displayRate
    }
    else {
      cardCurrency = sourceLabel
      cardSymbol = sourceSymbol
      cardPrice = node.price
    }
  }

  const formatMoney = (amount: number) => formatAmount(amount, cardSymbol)
  const expiry = Date.parse(node.expired_at)
  const hasExpiry = Number.isFinite(expiry)
  const remainingDays = hasExpiry
    ? (expiry - nowMs) / 86400000
    : 0
  const remainingValue = remainingDays > 36500
    ? cardPrice
    : hasExpiry && hasCycle && remainingDays > 0
      ? cardPrice * remainingDays / node.billing_cycle
      : 0
  const expiryText = hasExpiry
    ? getExpireText(node.expired_at)
    : '-'
  const expiryParts = EXPIRES_IN_DAYS.exec(expiryText)

  return [
    {
      label: '节点价格',
      value: hasPrice
        ? `${formatMoney(cardPrice)} ${cardCurrency}`
        : formatMoney(0),
      unit: hasPrice
        ? ` / ${getBillingCycleText(node.billing_cycle)}`
        : cardCurrency,
      icon: 'tabler:cash',
    },
    {
      label: '月均支出',
      value: hasCycle
        ? `${formatMoney(cardPrice / node.billing_cycle * 30)} ${cardCurrency}`
        : '不适用',
      unit: hasCycle ? ' / 月' : undefined,
      icon: 'tabler:receipt-2',
    },
    {
      label: '剩余时间',
      value: expiryParts?.[1] ?? expiryText,
      unit: expiryParts ? '天' : undefined,
      icon: 'tabler:calendar-dollar',
      valueClass: hasExpiry
        ? getExpireTextClass(node.expired_at)
        : undefined,
    },
    {
      label: '剩余价值',
      value: formatMoney(remainingValue),
      unit: cardCurrency,
      icon: 'tabler:coins',
    },
  ]
}
