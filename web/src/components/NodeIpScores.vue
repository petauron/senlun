<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { Icon } from '@iconify/vue'
import { computed, ref, watch } from 'vue'
import NodeIpVersionSwitch from '@/components/NodeIpVersionSwitch.vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { useNodeIpVersion } from '@/composables/useNodeIpVersion'
import { useAppStore } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { getSharedApi } from '@/utils/api'
import { getIpScoreGrade, nodeBadgeClass } from '@/utils/nodeBadges'

const props = defineProps<{ node: NodeData }>()
const app = useAppStore()
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
const displayScore = computed(() => score.value ?? '—')
const routeReport = computed(() => props.node.return_routes?.find(report => report.family === family.value))
watch(family, () => {
  message.value = ''
})

async function refresh() {
  const selectedFamily = family.value
  const uuid = props.node.uuid
  const ip = address.value
  if (!app.isLoggedIn || !ip || busy.value)
    return
  busy.value = true
  message.value = ''
  try {
    const result = await getSharedApi().post<{ data: { reputation: { trust_score: number | null } }, meta: { cache: string, updated_at: string, stale: boolean, warning: string | null } }>('admin/ip-info/v1/refresh', { uuid, ip })
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
        : result.meta.cache === 'hit'
          ? '评分仍在有效期内，已复用保存的结果。'
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
        class="ip-quality-trigger relative transition-shadow hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 before:absolute before:inset-x-0 before:-inset-y-2"
        :data-rating="grade.label" :class="[nodeBadgeClass, grade.tone]" :aria-label="`${node.name} IPv${family} IP 评分 ${displayScore} ${grade.label}，查看详情`"
        @click.stop @keydown.stop
      >
        <span class="font-bold tabular-nums"><span class="score-prefix">IP</span> {{ displayScore }}</span>
        <span class="font-medium">{{ grade.label }}</span>
      </button>
    </DialogTrigger>
    <DialogContent class="max-h-[85dvh] overflow-y-auto max-w-sm gap-4 rounded-2xl border-border/60 p-5" @click.stop>
      <DialogHeader class="text-left pr-7">
        <DialogTitle>IP 质量</DialogTitle>
        <DialogDescription>{{ node.name }} · IPv{{ family }}</DialogDescription>
      </DialogHeader>
      <div class="flex flex-wrap items-center justify-between gap-2 border-y border-border py-4">
        <div class="quality-reading flex items-baseline gap-2" :data-rating="grade.label">
          <strong class="text-4xl tabular-nums">{{ displayScore }}</strong>
          <span class="text-xs text-muted-foreground">/100</span>
          <span class="text-sm font-semibold">{{ grade.label }}</span>
        </div>
        <NodeIpVersionSwitch :node="node" />
      </div>
      <dl class="quality-meta">
        <dt>评分来源</dt><dd>Net.Coffee</dd>
        <dt>评分更新</dt><dd>{{ entry?.updated_at_unix_ms ? new Date(entry.updated_at_unix_ms).toLocaleString() : '尚未检测' }}</dd>
        <dt>回程更新</dt><dd>{{ routeReport?.checked_at_unix_ms ? new Date(routeReport.checked_at_unix_ms).toLocaleString() : '尚未检测' }}</dd>
      </dl>
      <p v-if="app.isLoggedIn && !address" class="text-xs text-muted-foreground">
        当前网络版本暂无可查询的地址。
      </p>
      <p v-if="message" role="status" class="text-xs text-muted-foreground">
        {{ message }}
      </p>
      <Button v-if="app.isLoggedIn" variant="secondary" class="w-full rounded-lg" :disabled="busy || !address" @click="refresh">
        <Icon icon="lucide:refresh-cw" class="mr-1.5 size-4" :class="{ 'animate-spin motion-reduce:animate-none': busy }" aria-hidden="true" />
        {{ busy ? '正在更新…' : '更新评分' }}
      </Button>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.quality-reading {
  color: var(--muted-foreground);
}
.quality-reading:is([data-rating='纯净'], [data-rating='极度纯净']) {
  color: var(--success);
}
.quality-reading:is([data-rating='良好'], [data-rating='中性']) {
  color: var(--warning);
}
.quality-reading[data-rating='可疑'] {
  color: var(--destructive);
}
.quality-meta {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 10px;
  font-size: 12px;
}
.quality-meta dt {
  color: var(--muted-foreground);
}
.quality-meta dd {
  text-align: right;
  font-variant-numeric: tabular-nums;
}
</style>
