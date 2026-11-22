<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import type { Client } from '@/utils/rpc'
import { Icon } from '@iconify/vue'
import { computed, onUnmounted, ref, watch } from 'vue'
import NodeIpVersionSwitch from '@/components/NodeIpVersionSwitch.vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { useNodeIpVersion } from '@/composables/useNodeIpVersion'
import { useAppStore } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { getSharedApi } from '@/utils/api'
import { getGoogleUnlockDisplay, getOrderedUnlockServices } from '@/utils/ipUnlock'
import { getIpScoreGrade, nodeBadgeClass } from '@/utils/nodeBadges'

const props = withDefaults(defineProps<{ node: NodeData, showChinaBadge?: boolean }>(), {
  showChinaBadge: true,
})
const app = useAppStore()
const open = ref(false)
const busy = ref(false)
const submittingUnlock = ref(false)
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
const grade = computed(() => getIpScoreGrade(score.value))
const displayScore = computed(() => score.value ?? '—')
const routeReport = computed(() => props.node.return_routes?.find(report => report.family === family.value))
const unlockReport = computed(() => props.node.ip_unlocks?.find(report => report.family === family.value))
const unlockActive = computed(() => props.node.ip_unlock_checks?.find(check => check.state === 'pending' || check.state === 'running'))
const unlockCurrent = computed(() => props.node.ip_unlock_checks?.find(check => check.family === family.value))
const unlockBusy = computed(() => submittingUnlock.value || !!unlockActive.value)
const unlockReady = computed(() => props.node.online && !!address.value && props.node.ip_unlock_capability === 'ready')
const googleInfo = computed(() => getGoogleUnlockDisplay(unlockReport.value))
const unlockServices = computed(() => getOrderedUnlockServices(unlockReport.value))
const visible = computed(() => Boolean(props.node.ip_scores?.length || props.node.ip_unlocks?.length || props.node.ipv4 || props.node.ipv6))

const unlockHint = computed(() => {
  if (!props.node.online)
    return '节点离线，暂时无法检测解锁'
  if (!address.value)
    return '当前网络版本暂无可查询的地址'
  if (props.node.ip_unlock_capability === 'upgrade_required' || props.node.ip_unlock_capability === undefined)
    return '请先更新该节点的监控探针以支持解锁检测'
  if (props.node.ip_unlock_capability !== 'ready')
    return '该节点暂不具备解锁检测条件'
  return '通过节点 Agent 轻量探测流媒体解锁与 Google 送中状态'
})

const unlockStatusLabel = computed(() => {
  if (unlockActive.value && unlockActive.value.family !== family.value)
    return `V${unlockActive.value.family} 探测中`
  if (unlockCurrent.value?.state === 'pending')
    return '排队中'
  if (unlockCurrent.value?.state === 'running')
    return '探测中'
  if (unlockCurrent.value?.state === 'failed') {
    return unlockReport.value
      ? '上次探测失败'
      : '探测未完成'
  }
  return ''
})

type UnlockState = Pick<Client, 'ip_unlocks' | 'ip_unlock_checks' | 'ip_unlock_capability'>

function applyUnlock(uuid: string, result: UnlockState) {
  const current = nodesStore.nodes.find(node => node.uuid === uuid)
  if (current) {
    current.ip_unlocks = result.ip_unlocks
    current.ip_unlock_checks = result.ip_unlock_checks
    current.ip_unlock_capability = result.ip_unlock_capability
  }
}

let timer: ReturnType<typeof setTimeout> | undefined
let disposed = false

function scheduleUnlockPoll() {
  clearTimeout(timer)
  if (disposed || !app.isLoggedIn || !unlockActive.value)
    return
  if (Date.now() - unlockActive.value.created_at_unix_ms > 5 * 60_000) {
    message.value = '解锁状态刷新已暂停，请重新打开查看。'
    return
  }
  const activeFamily = unlockActive.value.family
  timer = setTimeout(async () => {
    const uuid = props.node.uuid
    try {
      const result = await getSharedApi().get<UnlockState>(`admin/nodes/${encodeURIComponent(uuid)}/ip-unlock`)
      if (disposed || props.node.uuid !== uuid)
        return
      applyUnlock(uuid, result)
      const finished = result.ip_unlock_checks?.find(check => check.family === activeFamily)
      if (finished?.state === 'succeeded') {
        message.value = '解锁与 Google 定位状态已更新。'
      }
      else if (finished?.state === 'failed') {
        message.value = finished.error === 'address_mismatch'
          ? '节点实际出口地址与登记地址不一致，未写入结果。'
          : '解锁检测未完成，请稍后重试。'
      }
    }
    catch {
      if (!disposed && props.node.uuid === uuid)
        message.value = '解锁状态暂时无法刷新，已有结果保留。'
    }
    finally {
      scheduleUnlockPoll()
    }
  }, 3000)
}

watch([
  () => props.node.uuid,
  () => unlockActive.value?.state,
  () => app.isLoggedIn,
], scheduleUnlockPoll, { immediate: true })

watch(family, () => {
  message.value = ''
})

onUnmounted(() => {
  disposed = true
  clearTimeout(timer)
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
      },
    ]
    if (family.value !== selectedFamily)
      return
    message.value = result.meta.stale || result.meta.warning
      ? '暂未获取新结果，请稍后重试。'
      : result.data.reputation.trust_score === null
        ? '暂时没有可用评分。'
        : result.meta.cache === 'hit'
          ? '已复用保存的评分，检测时间保持不变。'
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

async function detectUnlock() {
  const selectedFamily = family.value
  const uuid = props.node.uuid
  if (!app.isLoggedIn || unlockBusy.value || !unlockReady.value)
    return
  submittingUnlock.value = true
  message.value = ''
  try {
    const result = await getSharedApi().post<UnlockState>(`admin/nodes/${encodeURIComponent(uuid)}/ip-unlock/check`, { family: selectedFamily })
    if (!disposed && props.node.uuid === uuid) {
      applyUnlock(uuid, result)
      message.value = '已发起轻量解锁探测，探针领取后返回结果（通常 15–30 秒）。'
    }
  }
  catch {
    if (!disposed && props.node.uuid === uuid && family.value === selectedFamily)
      message.value = '暂未发起解锁检测，请确认节点在线及探针已更新，或稍后重试。'
  }
  finally {
    submittingUnlock.value = false
  }
}
</script>

<template>
  <Dialog v-if="visible" v-model:open="open">
    <DialogTrigger as-child>
      <button
        type="button"
        class="ip-quality-trigger relative transition-shadow hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 before:absolute before:inset-x-0 before:-inset-y-2"
        :data-rating="grade.label" :class="[nodeBadgeClass, grade.tone]" :aria-label="`${node.name} IPv${family} IP 评分 ${displayScore} ${grade.label}${googleInfo.isChina ? '，Google 已送中' : ''}，查看详情`"
        @click.stop @keydown.stop
      >
        <span class="font-bold tabular-nums"><span class="score-prefix">IP</span> {{ displayScore }}</span>
        <span class="font-medium">{{ grade.label }}</span>
        <span
          v-if="showChinaBadge && googleInfo.isChina"
          class="ip-cn-flag rounded-full border border-red-500/35 bg-[#fce4ec] px-1.5 py-0.5 text-[10px] font-semibold leading-none text-[#c62828] dark:border-[#f85149]/35 dark:bg-[#2d1416] dark:text-[#f85149]"
        >送中</span>
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
      <div class="flex items-center justify-between gap-2 rounded-xl border px-3 py-2 text-xs" :class="googleInfo.tone">
        <div class="flex min-w-0 items-center gap-2">
          <span class="size-2 shrink-0 rounded-full" :class="googleInfo.dot" aria-hidden="true" />
          <div class="min-w-0">
            <div class="font-semibold">
              Google：{{ googleInfo.label }}
            </div>
            <div class="truncate text-[11px]">
              {{ googleInfo.hint }}
            </div>
          </div>
        </div>
        <span v-if="unlockStatusLabel" class="shrink-0 text-[11px] font-medium opacity-90">
          {{ unlockStatusLabel }}
        </span>
      </div>
      <div class="space-y-2">
        <div class="flex items-center justify-between text-xs">
          <span class="font-medium text-foreground">流媒体与 AI 解锁</span>
          <span class="text-[11px] text-muted-foreground">Agent 轻量探测</span>
        </div>
        <div class="grid grid-cols-2 gap-1.5">
          <div
            v-for="item in unlockServices"
            :key="item.id"
            class="flex items-center justify-between gap-1.5 rounded-lg border border-border/60 bg-muted/35 px-2.5 py-1.5 text-xs"
            :title="item.title"
          >
            <span class="truncate font-medium text-foreground">{{ item.name }}</span>
            <span class="shrink-0 rounded-full border px-1.5 py-0.5 text-[10px] font-semibold leading-none" :class="item.tone">
              {{ item.label }}
            </span>
          </div>
        </div>
      </div>
      <dl class="quality-meta">
        <dt>评分来源</dt><dd>Net.Coffee</dd>
        <dt>评分更新</dt><dd>{{ entry?.updated_at_unix_ms ? new Date(entry.updated_at_unix_ms).toLocaleString() : '尚未检测' }}</dd>
        <dt>解锁更新</dt><dd>{{ unlockReport?.checked_at_unix_ms ? new Date(unlockReport.checked_at_unix_ms).toLocaleString() : '尚未检测' }}</dd>
        <dt>回程更新</dt><dd>{{ routeReport?.checked_at_unix_ms ? new Date(routeReport.checked_at_unix_ms).toLocaleString() : '尚未检测' }}</dd>
      </dl>
      <p v-if="app.isLoggedIn && !address" class="text-xs text-muted-foreground">
        当前网络版本暂无可查询的地址。
      </p>
      <p v-else-if="app.isLoggedIn && !unlockReady" class="text-xs text-muted-foreground">
        {{ unlockHint }}
      </p>
      <p v-if="message" role="status" class="text-xs text-muted-foreground">
        {{ message }}
      </p>
      <div v-if="app.isLoggedIn" class="grid grid-cols-2 gap-2">
        <Button variant="secondary" class="w-full rounded-lg" :disabled="busy || !address" @click="refresh">
          <Icon icon="lucide:refresh-cw" class="mr-1.5 size-4" :class="{ 'animate-spin motion-reduce:animate-none': busy }" aria-hidden="true" />
          {{ busy ? '更新中…' : '更新评分' }}
        </Button>
        <Button variant="secondary" class="w-full rounded-lg" :title="unlockHint" :aria-busy="unlockBusy" :disabled="unlockBusy || !unlockReady" @click="detectUnlock">
          <Icon
            :icon="unlockBusy ? 'lucide:refresh-cw' : 'tabler:world-www'"
            class="mr-1.5 size-4"
            :class="{ 'animate-spin motion-reduce:animate-none': unlockBusy }"
            aria-hidden="true"
          />
          {{ unlockBusy ? '探测中…' : '检测解锁' }}
        </Button>
      </div>
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
