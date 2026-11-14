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
  return {
    ...carrier,
    label,
    route,
    grade: known
      ? getReturnRouteGrade(route?.label ?? '')
      : null,
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
          :title="`${row.name}：${row.label}${row.grade ? ` · ${row.grade.label}` : ''}`"
          :class="[
            homeCard ? 'h-[25px] rounded-full text-[11px]' : ['rounded-full py-1', compact ? 'px-0.5 text-[10px]' : 'px-1 text-[11px]'],
            (homeCard ? row.grade?.cardTone : row.grade?.tone) ?? nodeBadgeTones.unknown.tone,
            row.grade?.premium && 'shadow-sm ring-1 ring-[#228d5a]/25',
          ]"
        >
          <span v-if="homeCard" aria-hidden="true" class="w-3 shrink-0 rounded-full bg-current/10 text-center text-[9px] leading-[14px]">{{ row.short }}</span>
          <span v-else aria-hidden="true" class="flex size-3.5 shrink-0 items-center justify-center rounded-full text-[9px] font-bold" :class="row.tone">{{ row.short }}</span>
          <span :class="homeCard ? 'whitespace-nowrap font-[Arial,sans-serif] tabular-nums' : 'min-w-0 truncate'">{{ homeCard ? row.label.replaceAll('→', '›') : row.label }}</span>
          <span v-if="row.grade" class="sr-only">，{{ row.grade.label }}</span>
        </span>
      </button>
    </DialogTrigger>
    <DialogContent class="max-w-sm gap-4 rounded-2xl border-border/60 p-5" @click.stop>
      <DialogHeader>
        <DialogTitle>{{ node.name }} · 三网回程</DialogTitle>
        <DialogDescription>查看当前所选网络的上海三网检测结果。</DialogDescription>
      </DialogHeader>
      <NodeNetworkControls :node="node" />
      <div class="space-y-2">
        <div v-for="row in rows" :key="row.id" class="flex items-center gap-3 rounded-xl border p-3" :class="row.grade?.tone ?? nodeBadgeTones.unknown.tone">
          <span class="flex size-8 shrink-0 items-center justify-center rounded-full text-xs font-semibold" :class="row.tone">{{ row.short }}</span>
          <span class="flex-1 text-sm text-muted-foreground">{{ row.name }}</span>
          <span class="min-w-0 wrap-anywhere text-right text-sm font-semibold">{{ row.label }}</span>
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
