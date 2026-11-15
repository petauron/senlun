<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { computed, ref } from 'vue'
import NodeNetworkControls from '@/components/NodeNetworkControls.vue'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { useNodeIpVersion } from '@/composables/useNodeIpVersion'
import { getReturnRouteGrade } from '@/utils/nodeBadges'

const props = defineProps<{ node: NodeData, compact?: boolean }>()
const open = ref(false)
const { family } = useNodeIpVersion(() => props.node)
const report = computed(() => props.node.return_routes?.find(value => value.family === family.value))
const visible = computed(() => props.node.return_routes?.length || props.node.ipv4 || props.node.ipv6)
const carriers = [
  {
    id: 'telecom',
    name: '电信',
    short: '电',
  },
  {
    id: 'unicom',
    name: '联通',
    short: '联',
  },
  {
    id: 'mobile',
    name: '移动',
    short: '移',
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
        class="route-summary grid w-full min-w-0 grid-cols-3 items-center text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
        :class="{ 'route-summary-compact': compact }"
        @click.stop @keydown.stop
      >
        <span
          v-for="row in rows" :key="row.id"
          class="route-summary-cell flex min-w-0 items-center justify-center transition-colors"
          :data-carrier="row.id"
          :data-grade="row.grade?.label"
          :title="`${row.name}：${row.label}${row.grade ? ` · ${row.grade.label}` : ''}`"
        >
          <span aria-hidden="true" class="w-3 shrink-0 text-center">{{ row.short }}</span>
          <span class="whitespace-nowrap font-[Arial,sans-serif] tabular-nums">{{ row.label.replaceAll('→', '-') }}</span>
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
            <p v-if="row.grade" class="text-xs text-muted-foreground">
              {{ row.grade.label }}
            </p>
          </div>
        </div>
      </div>
      <p v-if="report?.address_changed" class="text-xs text-muted-foreground">
        地址已变更，请重新检测当前网络。
      </p>
      <p v-else-if="report?.stale" class="text-xs text-muted-foreground">
        上次结果已过期，请更新检测结果。
      </p>
      <details class="rounded-xl bg-muted/50 p-3 text-xs text-muted-foreground">
        <summary class="cursor-pointer font-medium text-foreground">
          检测详情
        </summary>
        <div class="mt-3 space-y-2 leading-relaxed">
          <template v-if="report">
            <p>IPv{{ family }} · 上海 · TCP · {{ report.source }}</p>
            <p>{{ new Date(report.checked_at_unix_ms).toLocaleString() }}</p>
            <p v-for="row in rows" :key="row.id" class="break-all">
              {{ row.name }}：{{ row.route?.target ?? '未检测' }} · {{ row.route?.label ?? '未知' }}
            </p>
          </template>
          <p v-else>
            当前网络版本尚无检测结果。
          </p>
        </div>
      </details>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
/* 回程摘要按三个等宽的数据栏显示。 */
.route-summary {
  gap: 0;
  grid-template-columns: repeat(3, minmax(max-content, 1fr));
  padding: 4px 0;
  border-radius: 0;
  border-block: 1px solid var(--border);
  background: transparent;
}
.route-summary-cell {
  position: relative;
  height: 24px;
  align-items: baseline;
  padding-block: 4px;
  line-height: 16px;
  gap: 3px;
  border: 0;
  border-radius: 0;
  background: transparent;
  box-shadow: none;
  font-weight: 500;
}
.route-summary-cell + .route-summary-cell::before {
  content: '';
  position: absolute;
  inset-inline-start: 0;
  top: 6px;
  height: 12px;
  width: 1px;
  background: var(--border);
}
.route-summary-cell {
  --carrier-hue: #5796e6;
  --route-ink: var(--muted-foreground);
}
[data-carrier='unicom'] {
  --carrier-hue: #ef7477;
}
[data-carrier='mobile'] {
  --carrier-hue: #91bd50;
}
[data-grade='顶级优化'] {
  --route-ink: oklch(from var(--warning) calc(l - 0.08) calc(c * 0.72) calc(h - 12));
}
[data-grade='一般优化'] {
  --route-ink: var(--success);
}
[data-grade='普通线路'] {
  --route-ink: var(--muted-foreground);
}
.route-summary-cell > span:first-child {
  color: var(--carrier-hue);
  font-size: 12px;
  font-weight: 600;
}
.route-summary-cell > span:nth-child(2) {
  color: var(--route-ink);
  font-size: 13px;
  font-weight: 550;
}
.route-summary-compact {
  grid-template-columns: repeat(3, minmax(0, 1fr));
  padding: 0;
  border: 0;
}
.route-summary-compact .route-summary-cell {
  height: 18px;
  padding: 0;
  gap: 2px;
}
.route-summary-compact .route-summary-cell::before {
  display: none;
}
.route-summary-compact .route-summary-cell > span {
  font-size: 11px;
  line-height: 14px;
}
.route-detail-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 0;
}
.carrier-name {
  font-size: 13px;
  font-weight: 600;
}
[data-carrier='telecom'] > .carrier-name {
  color: #5796e6;
}
[data-carrier='unicom'] > .carrier-name {
  color: #ef7477;
}
[data-carrier='mobile'] > .carrier-name {
  color: #91bd50;
}
.route-name {
  font-size: 15px;
  color: var(--muted-foreground);
  overflow-wrap: anywhere;
}
[data-grade='一般优化'] .route-name {
  color: var(--success);
}
[data-grade='顶级优化'] .route-name {
  color: oklch(from var(--warning) calc(l - 0.08) calc(c * 0.72) calc(h - 12));
}
</style>
