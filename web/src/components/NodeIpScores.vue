<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { Icon } from '@iconify/vue'
import { computed, ref, watch } from 'vue'
import NodeIpVersionSwitch from '@/components/NodeIpVersionSwitch.vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { useNodeIpVersion } from '@/composables/useNodeIpVersion'
import { useNodesStore } from '@/stores/nodes'
import { getSharedApi } from '@/utils/api'

const props = defineProps<{ node: NodeData }>()
const open = ref(false)
const busy = ref(false)
const message = ref('')
const nodesStore = useNodesStore()
const { family } = useNodeIpVersion(() => props.node)
const entry = computed(() => props.node.ip_scores?.find(value => value.family === family.value))
const address = computed(() => family.value === 4
  ? props.node.ipv4
  : props.node.ipv6)
const score = computed(() => {
  const value = entry.value?.score
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 100
    ? value
    : null
})
const levels = [
  {
    min: 95,
    max: 100,
    legendary: true,
    tone: 'border-amber-400/50 bg-linear-to-br from-amber-50 via-yellow-100 to-amber-200/80 text-amber-950 shadow-amber-300/25 dark:from-amber-950 dark:via-amber-900/70 dark:to-yellow-900/60 dark:text-amber-100',
  },
  {
    min: 80,
    max: 94,
    legendary: false,
    tone: 'border-violet-400/40 bg-linear-to-br from-violet-50 to-purple-100 text-violet-900 shadow-violet-300/20 dark:from-violet-950 dark:to-purple-900/60 dark:text-violet-200',
  },
  {
    min: 60,
    max: 79,
    legendary: false,
    tone: 'border-teal-400/40 bg-linear-to-br from-teal-50 to-emerald-100 text-teal-900 shadow-teal-300/20 dark:from-teal-950 dark:to-emerald-900/60 dark:text-teal-200',
  },
  {
    min: 40,
    max: 59,
    legendary: false,
    tone: 'border-orange-400/40 bg-linear-to-br from-orange-50 to-amber-100 text-orange-900 shadow-orange-300/20 dark:from-orange-950 dark:to-amber-900/60 dark:text-orange-200',
  },
  {
    min: 0,
    max: 39,
    legendary: false,
    tone: 'border-rose-400/40 bg-linear-to-br from-rose-50 to-red-100 text-rose-900 shadow-rose-300/20 dark:from-rose-950 dark:to-red-900/60 dark:text-rose-200',
  },
]
const neutral = { legendary: false, tone: 'border-border/70 bg-muted/70 text-muted-foreground' }
const grade = computed(() => {
  if (entry.value?.stale)
    return neutral
  const value = score.value
  return value === null
    ? neutral
    : levels.find(level => value >= level.min) ?? neutral
})
const displayScore = computed(() => entry.value?.stale
  ? '—'
  : score.value ?? '—')
const statusIcon = computed(() => {
  if (entry.value?.stale)
    return 'tabler:clock-hour-4'
  const value = score.value
  if (value === null)
    return 'lucide:info'
  return value < 40
    ? 'lucide:triangle-alert'
    : 'lucide:circle-check'
})
watch(family, () => {
  message.value = ''
})

async function refresh() {
  const selectedFamily = family.value
  const uuid = props.node.uuid
  const ip = address.value
  if (!ip || busy.value)
    return
  busy.value = true
  message.value = ''
  try {
    const result = await getSharedApi().post<{ data: { reputation: { trust_score: number | null } }, meta: { updated_at: string, stale: boolean, warning: string | null } }>('admin/ip-info/v1/refresh', { uuid, ip })
    const current = nodesStore.nodes.find(node => node.uuid === uuid)
    const currentAddress = selectedFamily === 4
      ? current?.ipv4
      : current?.ipv6
    if (!current || currentAddress !== ip)
      return
    current.ip_scores = [
      ...(current.ip_scores ?? []).filter(old => old.family !== selectedFamily),
      {
        family: selectedFamily,
        score: result.data.reputation.trust_score,
        provider: 'Net.Coffee',
        updated_at_unix_ms: Date.parse(result.meta.updated_at),
        stale: result.meta.stale,
      },
    ]
    if (family.value !== selectedFamily)
      return
    message.value = result.meta.stale || result.meta.warning
      ? '暂未获取新结果，请稍后重试。'
      : result.data.reputation.trust_score === null
        ? '暂时没有可用评分。'
        : '评分已更新。'
  }
  catch {
    if (family.value === selectedFamily)
      message.value = '评分暂时无法更新，请稍后重试。'
  }
  finally {
    busy.value = false
  }
}
</script>

<template>
  <Dialog v-if="node.ip_scores?.length" v-model:open="open">
    <DialogTrigger as-child>
      <button
        type="button"
        class="relative inline-flex min-h-6 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full border px-2 py-1 text-[10px] leading-none shadow-sm transition-shadow hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 before:absolute before:inset-x-0 before:-inset-y-2"
        :class="grade.tone" :aria-label="`${node.name} IPv${family} IP 评分 ${displayScore}，查看详情`"
        @click.stop @keydown.stop
      >
        <span v-if="grade.legendary" aria-hidden="true" class="size-1.5 rotate-45 rounded-[1px] border border-current bg-current/20" />
        <span class="font-bold tabular-nums">IP {{ displayScore }}</span>
      </button>
    </DialogTrigger>
    <DialogContent class="max-w-sm gap-5 rounded-2xl border-border/60 p-5" @click.stop>
      <DialogHeader>
        <DialogTitle>{{ node.name }} · IP 质量</DialogTitle>
        <DialogDescription>查看当前所选网络的评分与等级。</DialogDescription>
      </DialogHeader>
      <div class="flex justify-end">
        <NodeIpVersionSwitch :node="node" />
      </div>
      <div class="flex items-center justify-between rounded-2xl border p-5 shadow-sm" :class="grade.tone">
        <div class="space-y-1.5">
          <div class="text-xs opacity-70">IPv{{ family }} · IP 评分</div>
          <div class="flex items-center gap-2 text-lg font-semibold">
            <span v-if="grade.legendary" aria-hidden="true" class="size-3 rotate-45 rounded-xs border border-current bg-current/15" />
            <Icon v-else :icon="statusIcon" class="size-5" aria-hidden="true" />
            {{ entry?.stale ? '待更新' : score === null ? '待检测' : 'IP 评分' }}
          </div>
        </div>
        <div class="text-4xl font-bold tracking-tight tabular-nums">{{ displayScore }}<span class="ml-1 text-xs font-medium opacity-60">/100</span></div>
      </div>
      <div class="flex items-center justify-between gap-3 text-xs text-muted-foreground">
        <span>Net.Coffee</span>
        <span>{{ entry?.updated_at_unix_ms ? new Date(entry.updated_at_unix_ms).toLocaleString() : '尚未检测' }}</span>
      </div>
      <details class="rounded-xl bg-muted/50 p-3 text-xs text-muted-foreground">
        <summary class="cursor-pointer font-medium text-foreground">颜色说明</summary>
        <div class="mt-3 space-y-2">
          <div v-for="level in levels" :key="level.min" class="flex items-center justify-between">
            <span class="rounded-full border px-2 py-1" :class="level.tone">{{ level.min }}–{{ level.max }}</span>
          </div>
          <p class="pt-1 leading-relaxed">按 Net.Coffee 原始分数着色，仅供参考，不代表线路速度或无滥用保证。过期和缺失数据显示为灰色。</p>
        </div>
      </details>
      <p v-if="!address" class="text-xs text-muted-foreground">当前网络版本暂无可查询的地址。</p>
      <p v-if="message" role="status" class="text-xs text-muted-foreground">{{ message }}</p>
      <Button class="w-full rounded-xl" :disabled="busy || !address" @click="refresh">
        <Icon icon="lucide:refresh-cw" class="mr-1.5 size-4" :class="{ 'animate-spin motion-reduce:animate-none': busy }" aria-hidden="true" />
        {{ busy ? '正在更新…' : '更新评分' }}
      </Button>
    </DialogContent>
  </Dialog>
</template>
