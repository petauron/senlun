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
import { getIpScoreGrade, nodeBadgeClass } from '@/utils/nodeBadges'

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
const grade = computed(() => getIpScoreGrade(score.value, Boolean(entry.value?.stale)))
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
        class="relative transition-shadow hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 before:absolute before:inset-x-0 before:-inset-y-2"
        :class="[nodeBadgeClass, grade.tone]" :aria-label="`${node.name} IPv${family} IP 评分 ${displayScore}，查看详情`"
        @click.stop @keydown.stop
      >
        <span v-if="grade.legendary" aria-hidden="true" class="size-1.5 rotate-45 rounded-[1px] border border-current bg-current/20" />
        <span class="font-bold tabular-nums">IP {{ displayScore }}</span>
      </button>
    </DialogTrigger>
    <DialogContent class="max-w-sm gap-5 rounded-2xl border-border/60 p-5" @click.stop>
      <DialogHeader>
        <DialogTitle>{{ node.name }} · IP 质量</DialogTitle>
        <DialogDescription>查看当前所选网络的评分与更新时间。</DialogDescription>
      </DialogHeader>
      <div class="flex items-center justify-between gap-3">
        <span class="text-xs text-muted-foreground">当前网络</span>
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
      <p v-if="!address" class="text-xs text-muted-foreground">当前网络版本暂无可查询的地址。</p>
      <p v-if="message" role="status" class="text-xs text-muted-foreground">{{ message }}</p>
      <Button class="w-full rounded-xl" :disabled="busy || !address" @click="refresh">
        <Icon icon="lucide:refresh-cw" class="mr-1.5 size-4" :class="{ 'animate-spin motion-reduce:animate-none': busy }" aria-hidden="true" />
        {{ busy ? '正在更新…' : '更新评分' }}
      </Button>
    </DialogContent>
  </Dialog>
</template>
