<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import type { CurrencyCode } from '@/utils/financeHelper'
import { useMediaQuery, useNow } from '@vueuse/core'
import { computed, nextTick, ref, useId, watch } from 'vue'

import { DataTooltip } from '@/components/ui/data-tooltip'
import SelectControl from '@/components/ui/SelectControl.vue'
import { useExchangeRates } from '@/composables/useExchangeRates'
import { useAppStore } from '@/stores/app'
import {
  CURRENCY_SYMBOLS,
  FINANCE_CURRENCIES,
  getStoredFinanceCurrency,
  normalizeCurrency,
  setStoredFinanceCurrency,
  summarizeNodeValue,
} from '@/utils/financeHelper'
import { formatBytesPerSecondSplit, formatBytesSplit } from '@/utils/helper'
import { summarizeNodeCapacity } from '@/utils/nodeSummary'
import FlatMap from './NodeEarthMaps.vue'

const props = defineProps<{ nodes: NodeData[], globeNodes: NodeData[], transitionKey?: string }>()
const app = useAppStore()
const showRegions = computed(() => app.earthViewMode !== 'cards')
const desktop = useMediaQuery('(min-width: 641px)')
const trafficOpen = ref(false)
const resourcesOpen = ref(false)
const online = computed(() => props.nodes.filter(n => n.online))
function sum(key: 'net_total_up' | 'net_total_down' | 'net_out' | 'net_in', rows = props.nodes) {
  return rows.reduce((a, n) => a + (n[key] || 0), 0)
}
function fmt(bytes: number, speed = false) {
  return (speed ? formatBytesPerSecondSplit : formatBytesSplit)(bytes, { KB: 1, MB: 1, GB: 1, TB: 1 })
}
const stats = computed(() => {
  const { memory, disk } = summarizeNodeCapacity(props.nodes)
  return [
    { label: '累计流量', bytes: sum('net_total_up') + sum('net_total_down'), total: null },
    { label: '内存用量', bytes: memory.used, total: memory.total },
    { label: '磁盘用量', bytes: disk.used, total: disk.total },
  ]
})
const openFinanceCard = ref(false)

const financeId = useId()
const closeFinanceButton = ref<HTMLButtonElement | null>(null)

async function toggleFinanceCard(): Promise<void> {
  if (openFinanceCard.value) {
    closeFinanceCard()
    return
  }
  openFinanceCard.value = true
  await nextTick()
  closeFinanceButton.value?.focus()
}

function closeFinanceCard(): void {
  openFinanceCard.value = false
  document.getElementById(`${financeId}-trigger`)?.focus()
}

const financeCurrency = ref<CurrencyCode>(getStoredFinanceCurrency())
watch(financeCurrency, setStoredFinanceCurrency)
const { rates, date: rateDate, live: dailyRates, loading, feedback, refresh } = useExchangeRates()
const usedCurrencies = computed(() => {
  const used = new Set(props.nodes.filter(node => node.currency?.trim()).map(node => normalizeCurrency(node.currency)))
  return FINANCE_CURRENCIES.filter(code => used.has(code) && rates.value[code])
})
watch(
  usedCurrencies,
  (codes) => {
    if (codes.length && !codes.includes(financeCurrency.value))
      financeCurrency.value = codes[0] ?? 'CNY'
  },
  { immediate: true },
)
const now = useNow({ interval: 60000 })
const finance = computed(() => summarizeNodeValue(props.nodes, now.value.getTime(), rates.value))
function formatValue(value: number) {
  return (value * (rates.value[financeCurrency.value] ?? 1)).toLocaleString('zh-CN', {
    minimumFractionDigits: 0,
    maximumFractionDigits: 2,
  })
}
const formattedRemainingValue = computed(() => ({
  value: `${CURRENCY_SYMBOLS[financeCurrency.value]}${formatValue(finance.value.remaining)}`,
  unit: financeCurrency.value,
}))
const financeSummaryItems = computed(() => [
  {
    label: '总价值',
    value: formatValue(finance.value.total),
    symbol: CURRENCY_SYMBOLS[financeCurrency.value],
  },
  {
    label: '月均支出',
    value: formatValue(finance.value.monthly),
    symbol: CURRENCY_SYMBOLS[financeCurrency.value],
  },
  {
    label: '剩余价值',
    value: formatValue(finance.value.remaining),
    symbol: CURRENCY_SYMBOLS[financeCurrency.value],
  },
])
const exchangeRateRows = computed(() =>
  usedCurrencies.value
    .filter(currency => currency !== 'CNY')
    .map(currency => ({
      currency,
      rate: (1 / rates.value[currency]!).toLocaleString('zh-CN', { maximumSignificantDigits: 4, useGrouping: false }),
      symbol: CURRENCY_SYMBOLS[currency],
    })),
)
const financeDisclosure = computed(() =>
  [
    !exchangeRateRows.value.length
      ? ''
      : dailyRates.value
        ? `Frankfurter 每日汇率（${rateDate.value}）；休市时显示最近交易日。`
        : '当前估算使用内置参考汇率。',
    '月均按 30 天折算；剩余按未到期时长折算。',
    finance.value.excluded
      ? `${finance.value.excluded} 个节点因资料不完整，未计入部分估算。`
      : '',
  ]
    .filter(Boolean)
    .join(' '),
)
</script>

<template>
  <section class="overview">
    <div class="headline">
      <div>
        <label>在线节点</label><strong>{{ online.length }}<small> / {{ nodes.length }}</small></strong>
      </div>
      <div>
        <label>总速率</label><strong><i>↑</i> {{ fmt(sum('net_out', online), true).value
        }}<small v-if="fmt(sum('net_out', online), true).unit !== fmt(sum('net_in', online), true).unit">{{
          fmt(sum('net_out', online), true).unit
        }}</small>
          <i>↓</i> {{ fmt(sum('net_in', online), true).value }}<small>{{ fmt(sum('net_in', online), true).unit }}</small></strong>
      </div>
    </div>
    <button v-if="showRegions && !desktop" class="disclosure" :aria-expanded="trafficOpen" @click="trafficOpen = !trafficOpen">
      地区流量 <span>{{ trafficOpen ? '−' : '+' }}</span>
    </button>
    <FlatMap v-if="showRegions && (desktop || trafficOpen)" :desktop="desktop" :nodes="globeNodes" />
    <button v-if="!desktop" class="disclosure" :aria-expanded="resourcesOpen" @click="resourcesOpen = !resourcesOpen">
      资源与费用 <span>{{ resourcesOpen ? '−' : '+' }}</span>
    </button>
    <div v-if="desktop || resourcesOpen" class="resources">
      <div class="secondary">
        <div v-for="{ label, bytes, total } in stats" :key="label">
          <DataTooltip
            v-if="label === '累计流量'"
            as="button"
            :content="
              `↑ ${
                fmt(sum('net_total_up')).value
              } ${
                fmt(sum('net_total_up')).unit
              } ↓ ${
                fmt(sum('net_total_down')).value
              } ${
                fmt(sum('net_total_down')).unit}`
            "
            placement="bottom"
          >
            <label>{{ label }} ›</label><b>{{ fmt(bytes).value }} {{ fmt(bytes).unit }}</b>
          </DataTooltip><template v-else>
            <label>{{ label }}</label><b>{{ fmt(bytes).value }} {{ fmt(bytes).unit }} / {{ fmt(total ?? 0).value }} {{ fmt(total ?? 0).unit }}</b>
          </template>
        </div>
        <button
          :id="`${financeId}-trigger`" aria-label="查看剩余价值详情"
          :aria-expanded="openFinanceCard"
          :aria-controls="`${financeId}-panel`"
          @click="toggleFinanceCard"
        >
          <label>剩余价值 ›</label><b>{{ formattedRemainingValue.value }} {{ formattedRemainingValue.unit }}</b>
        </button>
      </div>
      <section
        v-if="openFinanceCard"
        :id="`${financeId}-panel`"
        class="finance"
        aria-label="剩余价值详情"
        @keydown.esc="closeFinanceCard"
      >
        <header>
          <strong>价值详情</strong><button ref="closeFinanceButton" aria-label="关闭价值详情" @click="closeFinanceCard">
            关闭
          </button>
        </header>
        <div class="amounts">
          <div v-for="item in financeSummaryItems" :key="item.label">
            <label>{{ item.label }}</label><b>{{ item.symbol }}{{ item.value }}</b>
          </div>
        </div>
        <div v-if="exchangeRateRows.length" class="curr">
          <div class="rate-tools">
            <span>{{ dailyRates ? `每日汇率 · ${rateDate}` : '内置参考汇率' }}</span><button type="button" :disabled="loading" aria-label="刷新汇率" @click="refresh">
              {{ loading ? '刷新中…' : '刷新' }}
            </button>
          </div>
          <SelectControl
            v-model="financeCurrency"
            aria-label="切换金额显示币种"
            :options="usedCurrencies.map((code) => ({ value: code, label: code }))"
          />
        </div>
        <small aria-live="polite">{{ feedback }}</small>
        <div v-if="exchangeRateRows.length" class="rates">
          <span v-for="r in exchangeRateRows" :key="r.currency">1 {{ r.currency }} = {{ r.rate }} 人民币</span>
        </div>
        <p>{{ financeDisclosure }}</p>
      </section>
    </div>
  </section>
</template>

<style scoped>
.overview {
  margin: 4px 16px 16px;
  padding: 18px 24px;
  background: #19222d;
  border: 1px solid #ffffff15;
  border-radius: 14px;
  color: #edf1f6;
  font-variant-numeric: tabular-nums;
}
.headline {
  display: flex;
  align-items: center;
  gap: 44px;
  padding-bottom: 12px;
}
.headline label,
.secondary label {
  display: block;
  font-size: 12px;
  color: #a8b4c2;
}
.headline strong {
  display: block;
  font-size: 28px;
  line-height: 1.4;
  letter-spacing: -0.035em;
}
.headline i {
  font-size: 0.65em;
  color: #a8b4c2;
  font-style: normal;
}
.headline small {
  font-size: 13px;
  color: #9fabba;
  font-weight: 500;
  letter-spacing: 0;
}
.secondary {
  display: flex;
  gap: 32px;
  align-items: center;
  border-top: 1px solid #ffffff16;
  padding-top: 12px;
}
.secondary b {
  font-size: 14px;
}
.disclosure {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 40px;
  width: 100%;
  border-top: 1px solid #ffffff18;
  font-size: 13px;
  font-weight: 550;
}
.disclosure span {
  color: #9baebb;
}
.resources {
  margin-top: 12px;
}
@media (max-width: 640px) {
  .overview {
    padding: 12px 14px 3px;
  }
  .headline {
    justify-content: space-between;
    gap: 8px;
  }
  .headline strong {
    font-size: 21px;
  }
  .headline small {
    font-size: 10px;
  }
  .resources {
    margin: 0 0 12px;
  }
  .secondary {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    border: none;
    padding: 4px 0;
  }
}
.secondary button {
  text-align: left;
  cursor: pointer;
}
.finance {
  margin-top: 16px;
  padding: 14px;
  border-radius: 10px;
  background: #111923;
  color: #d3dce4;
  font-size: 12px;
}
.finance header,
.curr,
.amounts {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 5px;
}
.amounts,
.curr {
  margin-top: 10px;
}
.rates {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 3px;
}
.finance p {
  color: #9aa7b7;
  margin-top: 8px;
}
.finance header button {
  cursor: pointer;
}
.rate-tools {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.rate-tools button {
  border: 1px solid #465264;
  border-radius: 5px;
  padding: 4px 7px;
  cursor: pointer;
}
.rate-tools button:disabled {
  opacity: 0.6;
}
.finance small {
  color: #aab7c4;
}
.overview {
  color: var(--card-foreground);
  background: var(--card);
  border-color: var(--border);
}
.overview :is(label, .headline i, .headline small, .finance small, .finance p, .disclosure span) {
  color: var(--muted-foreground);
}
.overview :is(.secondary, .disclosure, .rate-tools button) {
  border-color: var(--border);
}
.overview .finance {
  background: var(--muted);
  color: var(--foreground);
}
</style>
