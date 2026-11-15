import type { CurrencyCode } from '@/utils/financeHelper'
import { useNow } from '@vueuse/core'
import dayjs from 'dayjs'
import { computed, onBeforeUnmount, shallowRef, watch } from 'vue'
import { useAppStore } from '@/stores/app'
import { CURRENCY_SYMBOLS, REFERENCE_EXCHANGE_RATES } from '@/utils/financeHelper'

const datePattern = /^\d{4}-\d{2}-\d{2}$/
const cacheKey = 'senlun:frankfurter-cny:v1'
type Rates = Partial<Record<CurrencyCode, number>>
function parseRates(raw: unknown): Rates | null {
  if (!raw || typeof raw !== 'object')
    return null
  const rates: Rates = { CNY: 1 }
  for (const code of Object.keys(CURRENCY_SYMBOLS) as CurrencyCode[]) {
    const rate = (raw as Record<string, unknown>)[code]
    if (typeof rate === 'number' && Number.isFinite(rate) && rate > 0)
      rates[code] = rate
  }
  return rates.USD && rates.EUR
    ? rates
    : null
}

export function useExchangeRates() {
  const app = useAppStore()
  const rates = shallowRef<Rates>(REFERENCE_EXCHANGE_RATES)
  const date = shallowRef('')
  const live = computed(() => Boolean(date.value))
  const clock = useNow({ interval: 60000 })
  const day = computed(() => dayjs(clock.value).format('YYYY-MM-DD'))
  let controller: AbortController | undefined
  const loading = shallowRef(false)
  const feedback = shallowRef('')
  async function load(force = false) {
    controller?.abort()
    controller = undefined
    loading.value = false
    feedback.value = ''
    if (app.publicSettings?.theme_settings?.dailyExchangeRates !== true) {
      rates.value = REFERENCE_EXCHANGE_RATES
      date.value = ''
      return
    }
    const today = day.value
    if (!force) {
      try {
        const raw = localStorage.getItem(cacheKey)
        if (raw && raw.length < 8192) {
          const cache = JSON.parse(raw)
          const parsed = parseRates(cache.rates)
          if (cache.fetched === today && typeof cache.date === 'string' && parsed) {
            rates.value = parsed
            date.value = cache.date
            return
          }
        }
      }
      catch { /* Storage is optional; never prevent rendering. */ }
    }
    const request = new AbortController()
    loading.value = true
    controller = request
    const timer = setTimeout(() => request.abort(), 5000)
    try {
      const response = await fetch('https://api.frankfurter.dev/v1/latest?base=CNY', {
        signal: request.signal,
        cache: force ? 'no-store' : 'default',
        credentials: 'omit',
        referrerPolicy: 'no-referrer',
      })
      if (!response.ok)
        throw new Error('Exchange rate request failed')
      const payload = await response.json()
      const parsed = parseRates(payload.rates)
      if (request.signal.aborted || !parsed || payload.base !== 'CNY' || !datePattern.test(payload.date))
        throw new Error('Invalid exchange rate response')
      if (force)
        feedback.value = '已刷新'
      rates.value = parsed
      date.value = payload.date
      try {
        localStorage.setItem(cacheKey, JSON.stringify({ fetched: today, date: payload.date, rates: parsed }))
      }
      catch { /* Private browsing may disable storage. */ }
    }
    catch {
      if (force && controller === request)
        feedback.value = '刷新失败，保留当前汇率'
    }
    finally {
      clearTimeout(timer)
      if (controller === request)
        loading.value = false
    }
  }
  watch(() => [app.publicSettings?.theme_settings?.dailyExchangeRates, day.value], () => load(), { immediate: true })
  onBeforeUnmount(() => controller?.abort())
  return {
    rates,
    date,
    live,
    loading,
    feedback,
    refresh: () => load(true),
  }
}
