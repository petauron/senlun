<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { computed, ref } from 'vue'
import NodeNetworkControls from '@/components/NodeNetworkControls.vue'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { useNodeIpVersion } from '@/composables/useNodeIpVersion'

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
          ? route.label.replace(/^10099->/, '').replaceAll('->', '→')
          : '未知'
  return {
    ...carrier,
    label,
    route,
    known,
    premium: known && /CN2|9929|CMIN2|GIA|CU2/i.test(route?.label ?? ''),
  }
}))
const summary = computed(() => `${props.node.name} IPv${family.value} 三网回程：${rows.value.map(row => `${row.name} ${row.label}`).join('，')}，查看详情`)
</script>

<template>
  <Dialog v-if="visible" v-model:open="open">
    <DialogTrigger as-child>
      <button
        type="button" :aria-label="summary"
        class="grid min-h-9 w-full grid-cols-3 items-center gap-1 rounded-lg text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
        @click.stop @keydown.stop
      >
        <span
          v-for="row in rows" :key="row.id"
          class="flex min-w-0 items-center justify-center gap-1 rounded-full border py-1 font-semibold transition-colors"
          :class="[
            compact ? 'px-1 text-[10px]' : 'px-1.5 text-[11px]',
            row.known ? 'border-indigo-300/50 bg-linear-to-b from-background/90 to-indigo-50/80 text-slate-700 shadow-sm shadow-indigo-300/15 hover:border-indigo-400/70 dark:border-indigo-400/30 dark:from-slate-800/90 dark:to-indigo-950/70 dark:text-slate-200' : 'border-border/60 bg-muted/40 text-muted-foreground',
            row.premium && 'ring-1 ring-indigo-400/15',
          ]"
        >
          <span aria-hidden="true" class="flex size-3.5 shrink-0 items-center justify-center rounded-full text-[9px] font-bold" :class="row.known ? row.tone : 'bg-muted text-muted-foreground'">{{ row.short }}</span>
          <span class="min-w-0 truncate">{{ row.label }}</span>
          <span v-if="row.premium" aria-hidden="true" class="size-1.5 shrink-0 rotate-45 rounded-[1px] border border-indigo-500/60 bg-indigo-400/20 dark:border-indigo-300/70" />
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
        <div v-for="row in rows" :key="row.id" class="flex items-center gap-3 rounded-xl border border-border/60 bg-muted/25 p-3">
          <span class="flex size-8 shrink-0 items-center justify-center rounded-full text-xs font-semibold" :class="row.known ? row.tone : 'bg-muted text-muted-foreground'">{{ row.short }}</span>
          <span class="flex-1 text-sm text-muted-foreground">{{ row.name }}</span>
          <span class="min-w-0 truncate text-sm font-semibold" :class="row.known ? 'text-foreground' : 'text-muted-foreground'">{{ row.label }}</span>
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
          <p>仅代表本次上海回程，七天后需更新；不代表其他地区或实际下载速度。</p>
        </div>
      </details>
    </DialogContent>
  </Dialog>
</template>
