<script setup lang="ts">
import type { PeerLatency } from '@/utils/rpc'
import { ref, watch } from 'vue'
import { CardX } from '@/components/ui/card-x'
import { getSharedApi } from '@/utils/api'

const props = defineProps<{ nodeId: string }>()
const rows = ref<PeerLatency[]>([])
const loadState = ref<'loading' | 'ready' | 'error'>('loading')
const labels: Record<PeerLatency['status'], string> = {
  ready: '正常',
  pending: '待采样',
  offline: '离线',
  stale: '已过期',
  unconfigured: '未配置',
  timeout: '超时',
  unavailable: '探测不可用',
  failed: '探测失败',
}

watch(() => props.nodeId, (nodeId, _, onCleanup) => {
  const controller = new AbortController()
  let timer: number | undefined
  rows.value = []
  loadState.value = 'loading'
  onCleanup(() => {
    controller.abort()
    window.clearTimeout(timer)
  })

  async function refresh() {
    try {
      const result = await getSharedApi().get<PeerLatency[]>(`v1/nodes/${encodeURIComponent(nodeId)}/peer-latency`, controller.signal)
      if (!controller.signal.aborted) {
        rows.value = result
        loadState.value = 'ready'
      }
    }
    catch {
      if (!controller.signal.aborted) {
        rows.value = []
        loadState.value = 'error'
      }
    }
    if (!controller.signal.aborted)
      timer = window.setTimeout(refresh, 30_000)
  }

  void refresh()
}, { immediate: true })
</script>

<template>
  <CardX title="节点互测">
    <p v-if="loadState !== 'ready'" role="status" class="text-sm text-muted-foreground">
      {{ loadState === 'loading' ? '正在加载…' : '暂时无法读取节点互测结果' }}
    </p>
    <p v-else-if="!rows.length" class="text-sm text-muted-foreground">暂无其他节点</p>
    <div v-else class="overflow-x-auto">
      <table class="w-full text-left text-sm">
        <thead class="text-muted-foreground">
          <tr>
            <th scope="col" class="p-2">对端节点</th>
            <th scope="col" class="p-2">协议</th>
            <th scope="col" class="p-2">地址族</th>
            <th scope="col" class="p-2">延迟</th>
            <th scope="col" class="p-2">状态</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rows" :key="row.node_id" class="border-t border-border">
            <th scope="row" class="p-2 font-medium break-all">{{ row.name }}</th>
            <td class="p-2">{{ row.kind?.toUpperCase() ?? '—' }}</td>
            <td class="p-2">{{ row.family ? `IPv${row.family}` : '—' }}</td>
            <td class="p-2 whitespace-nowrap tabular-nums">{{ row.latency_ms !== null ? `${row.latency_ms} ms` : '—' }}</td>
            <td class="p-2 whitespace-nowrap">{{ labels[row.status] }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </CardX>
</template>
