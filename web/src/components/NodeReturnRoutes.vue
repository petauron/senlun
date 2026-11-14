<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { computed, ref } from 'vue'
import NodeNetworkControls from '@/components/NodeNetworkControls.vue'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { useNodeIpVersion } from '@/composables/useNodeIpVersion'
import { getReturnRouteGrade, nodeBadgeTones } from '@/utils/nodeBadges'

const props = defineProps<{ node: NodeData, compact?: boolean, homeCard?: boolean }>()
const open = ref(false)
const { family } = useNodeIpVersion(() => props.node)
const report = computed(() => props.node.return_routes?.find(value => value.family === family.value))
const visible = computed(() => props.node.return_routes?.length || props.node.ipv4 || props.node.ipv6)
const carriers = [
  {
    id: 'telecom',
    name: '电信',
    short: '电',
    tone: 'bg-violet-500/15 text-violet-700 dark:text-violet-300',
  },
  {
    id: 'unicom',
    name: '联通',
    short: '联',
    tone: 'bg-rose-500/15 text-rose-700 dark:text-rose-300',
  },
  {
    id: 'mobile',
    name: '移动',
    short: '移',
    tone: 'bg-teal-500/15 text-teal-700 dark:text-teal-300',
  },
]
const rows = computed(() => carriers.map((carrier) => {
  const route = report.value?.carriers.find(value => value.carrier === carrier.id)
  const known = Boolean(report.value && !report.value.stale && !report.value.address_changed && route?.status === 'ok')
  const label = !report.value
    ? '待检测'
    : report.value.address_changed
      ? '需复测'
      : report.value.stale
        ? '待更新'
        : route?.status === 'ok'
          ? route.label.replaceAll('->', '→')
          : '未知'
  const grade = known
    ? getReturnRouteGrade(label)
    : null
  return {
    ...carrier,
    label,
    route,
    grade,
  }
}))
const summary = computed(() => {
  const routes = rows.value.map(row => [row.name, row.label, row.grade?.label].filter(Boolean).join(' ')).join('，')
  return `${props.node.name} IPv${family.value} 三网回程：${routes}，查看详情`
})
</script>

<template>
  <Dialog v-if="visible" v-model:open="open">
    <DialogTrigger as-child>
      <button
        type="button" :aria-label="summary"
        class="grid w-full min-w-0 grid-cols-3 items-center gap-1 rounded-lg text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
        @click.stop @keydown.stop
      >
        <span
          v-for="row in rows" :key="row.id"
          class="flex min-w-0 items-center justify-center gap-0.5 border font-semibold transition-colors"
          :data-carrier="homeCard ? row.id : undefined"
          :data-grade="homeCard ? row.grade?.label : undefined"
          :title="`${row.name}：${row.label}${row.grade ? ` · ${row.grade.label}` : ''}`"
          :class="[
            homeCard ? 'route-summary-cell h-[25px] rounded-full text-[11px]' : ['rounded-full py-1', compact ? 'px-0.5 text-[10px]' : 'px-1 text-[11px]'],
            !homeCard && (row.grade?.tone ?? nodeBadgeTones.unknown.tone),
            !homeCard && row.grade?.premium && 'shadow-sm ring-1 ring-[#228d5a]/25',
          ]"
        >
          <span v-if="homeCard" aria-hidden="true" class="w-3 shrink-0 rounded-sm text-center text-[9px] leading-[14px]">{{ row.short }}</span>
          <span v-else aria-hidden="true" class="flex size-3.5 shrink-0 items-center justify-center rounded-full text-[9px] font-bold" :class="row.tone">{{ row.short }}</span>
          <span :class="homeCard ? 'whitespace-nowrap font-[Arial,sans-serif] tabular-nums' : 'min-w-0 truncate'">{{ homeCard ? row.label.replaceAll('→', '-') : row.label }}</span>
          <span v-if="row.grade" class="sr-only">，{{ row.grade.label }}</span>
        </span>
      </button>
    </DialogTrigger>
    <DialogContent class="max-h-[85dvh] overflow-y-auto max-w-sm gap-3 rounded-2xl border-border/60 p-5" @click.stop>
      <DialogHeader class="pr-7 text-left">
        <DialogTitle>三网回程</DialogTitle>
        <DialogDescription>{{ node.name }} · IPv{{ family }} · 上海</DialogDescription>
      </DialogHeader>
      <NodeNetworkControls :node="node" />
      <div class="divide-y divide-border border-y border-border">
        <div v-for="row in rows" :key="row.id" class="route-detail-row" :data-carrier="row.id" :data-grade="row.grade?.label">
          <span class="carrier-name">{{ row.name }}</span>
          <div class="min-w-0 text-right">
            <strong class="route-name">{{ row.label }}</strong>
            <p v-if="row.grade" class="text-xs text-muted-foreground">{{ row.grade.label }}</p>
          </div>
        </div>
      </div>
      <p v-if="report?.address_changed" class="text-xs text-muted-foreground">地址已变更，请重新检测当前网络。</p>
      <p v-else-if="report?.stale" class="text-xs text-muted-foreground">上次结果已过期，请更新检测结果。</p>
      <details class="rounded-xl bg-muted/50 p-3 text-xs text-muted-foreground">
        <summary class="cursor-pointer font-medium text-foreground">检测详情</summary>
        <div class="mt-3 space-y-2 leading-relaxed">
          <template v-if="report">
            <p>IPv{{ family }} · 上海 · TCP · {{ report.source }}</p>
            <p>{{ new Date(report.checked_at_unix_ms).toLocaleString() }}</p>
            <p v-for="row in rows" :key="row.id" class="break-all">{{ row.name }}：{{ row.route?.target ?? '未检测' }} · {{ row.route?.label ?? '未知' }}</p>
          </template>
          <p v-else>当前网络版本尚无检测结果。</p>
        </div>
      </details>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.route-detail-row {display:flex;align-items:center;justify-content:space-between;gap:12px;padding:10px 0;}
.carrier-name {font-size:13px;font-weight:600;}
[data-carrier="telecom"] > .carrier-name {color:#5796E6;}
[data-carrier="unicom"] > .carrier-name {color:#EF7477;}
[data-carrier="mobile"] > .carrier-name {color:#91BD50;}
.route-name {font-size:15px;color:var(--muted-foreground);overflow-wrap:anywhere;}
[data-grade="一般优化"] .route-name {color:var(--success);}
[data-grade="顶级优化"] .route-name {color:oklch(from var(--warning) calc(l - 0.08) calc(c * 0.72) calc(h - 12));}
</style>
