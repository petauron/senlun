<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import type { Client } from '@/utils/rpc'
import { Icon } from '@iconify/vue'
import { computed, onUnmounted, ref, watch } from 'vue'
import NodeIpVersionSwitch from '@/components/NodeIpVersionSwitch.vue'
import { Button } from '@/components/ui/button'
import { useNodeIpVersion } from '@/composables/useNodeIpVersion'
import { useAppStore } from '@/stores/app'
import { useNodesStore } from '@/stores/nodes'
import { getSharedApi } from '@/utils/api'

const props = defineProps<{ node: NodeData }>()
const app = useAppStore()
const nodes = useNodesStore()
const { family } = useNodeIpVersion(() => props.node)
const submitting = ref(false)
const message = ref('')
const active = computed(() => props.node.return_route_checks?.find(value => value.state === 'pending' || value.state === 'running'))
const current = computed(() => props.node.return_route_checks?.find(value => value.family === family.value))
const report = computed(() => props.node.return_routes?.find(value => value.family === family.value))
const address = computed(() => family.value === 4
  ? props.node.ipv4
  : props.node.ipv6)
const busy = computed(() => submitting.value || !!active.value)
const ready = computed(() => props.node.online && !!address.value && props.node.return_route_capability === 'ready')
const hint = computed(() => {
  if (!props.node.online)
    return '节点离线，暂时无法检测'
  if (!address.value)
    return '当前网络版本尚未配置地址'
  if (props.node.return_route_capability === 'upgrade_required')
    return '请先更新该节点的监控探针'
  if (props.node.return_route_capability === 'unavailable')
    return '该节点暂不具备回程检测条件'
  return '检测当前网络版本的上海三网回程，并更新 IP 评分'
})
const status = computed(() => {
  if (active.value && active.value.family !== family.value)
    return `V${active.value.family} 检测中`
  if (current.value?.state === 'pending')
    return '排队中'
  if (current.value?.state === 'running')
    return '检测中'
  if (current.value?.state === 'failed') {
    return report.value
      ? '检测失败 · 显示上次结果'
      : '检测未完成'
  }
  if (report.value?.address_changed)
    return '地址已变更 · 需重新检测'
  if (report.value?.stale)
    return '结果已过期'
  if (report.value)
    return `更新于 ${new Date(report.value.checked_at_unix_ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`
  return '尚未检测'
})

type NetworkState = Pick<Client, 'return_routes' | 'return_route_checks' | 'return_route_capability'>
function apply(uuid: string, result: NetworkState) {
  const node = nodes.nodes.find(value => value.uuid === uuid)
  if (node) {
    node.return_routes = result.return_routes
    node.return_route_checks = result.return_route_checks
    node.return_route_capability = result.return_route_capability
  }
}

let timer: ReturnType<typeof setTimeout> | undefined
let disposed = false
function schedule() {
  clearTimeout(timer)
  if (disposed || !app.isLoggedIn || !active.value)
    return
  // Poll only an explicit live job. No page-load probe or background fleet scan.
  timer = setTimeout(async () => {
    const uuid = props.node.uuid
    try {
      const result = await getSharedApi().get<NetworkState>(`admin/nodes/${encodeURIComponent(uuid)}/return-routes`)
      if (!disposed && props.node.uuid === uuid)
        apply(uuid, result)
    }
    catch {
      if (!disposed)
        message.value = '检测状态暂时无法刷新，已有结果保留。'
    }
    finally {
      schedule()
    }
  }, 3000)
}
watch(() => [props.node.uuid, active.value?.state, app.isLoggedIn], schedule, { immediate: true })
watch(family, () => {
  message.value = ''
})
onUnmounted(() => {
  disposed = true
  clearTimeout(timer)
})

async function detect() {
  if (busy.value || !ready.value)
    return
  const uuid = props.node.uuid
  const selectedFamily = family.value
  const ip = address.value
  submitting.value = true
  message.value = ''
  try {
    const result = await getSharedApi().post<NetworkState>(`admin/nodes/${encodeURIComponent(uuid)}/return-routes/check`, { family: selectedFamily })
    apply(uuid, result)
    try {
      const score = await getSharedApi().post<{ data: { reputation: { trust_score: number | null } }, meta: { updated_at: string, stale: boolean } }>('admin/ip-info/v1/refresh', { uuid, ip })
      const node = nodes.nodes.find(value => value.uuid === uuid)
      const nowAddress = selectedFamily === 4
        ? node?.ipv4
        : node?.ipv6
      if (node && nowAddress === ip) {
        node.ip_scores = [
          ...(node.ip_scores ?? []).filter(value => value.family !== selectedFamily),
          {
            family: selectedFamily,
            score: score.data.reputation.trust_score,
            provider: 'Net.Coffee',
            updated_at_unix_ms: Date.parse(score.meta.updated_at),
            stale: score.meta.stale,
          },
        ]
      }
    }
    catch {
      if (!disposed && family.value === selectedFamily)
        message.value = '回程已排队，IP 评分暂未更新。'
    }
  }
  catch {
    if (!disposed && props.node.uuid === uuid && family.value === selectedFamily)
      message.value = '暂未发起检测，请确认节点在线及探针已更新，或稍后重试。'
  }
  finally {
    submitting.value = false
    schedule()
  }
}
</script>

<template>
  <div class="space-y-1.5 py-1" @click.stop @keydown.stop>
    <div class="flex min-h-8 items-center justify-between gap-2">
      <div class="min-w-0 flex-1">
        <div class="text-[11px] font-medium text-foreground">网络质量</div>
        <div class="truncate text-[10px] text-muted-foreground" role="status">{{ status }}</div>
      </div>
      <div class="flex shrink-0 items-center gap-1">
        <NodeIpVersionSwitch :node="node" />
        <Button
          v-if="app.isLoggedIn" variant="ghost" size="xs" class="h-7 rounded-full px-2 text-[10px]"
          :disabled="busy || !ready" :title="hint" :aria-label="`${node.name} 检测 IPv${family} 网络质量`"
          @click="detect"
        >
          <Icon icon="lucide:refresh-cw" class="mr-1 size-3" :class="{ 'animate-spin motion-reduce:animate-none': busy }" aria-hidden="true" />
          {{ busy ? '检测中' : '检测' }}
        </Button>
      </div>
    </div>
    <p v-if="message && app.isLoggedIn" role="status" class="text-[10px] leading-relaxed text-muted-foreground">{{ message }}</p>
  </div>
</template>
