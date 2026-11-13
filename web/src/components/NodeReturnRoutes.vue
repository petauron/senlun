<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import type { ReturnRoute } from '@/utils/rpc'
import { useNodesStore } from '@/stores/nodes'
import { ref } from 'vue'
import { getSharedApi } from '@/utils/api'

const props = defineProps<{ node: NodeData }>()
const busy = ref(false)
const message = ref('')
const nodesStore = useNodesStore()
const names: Record<string, string> = { telecom: '电信', unicom: '联通', mobile: '移动' }
async function importReport(event: Event) {
  const uuid = props.node.uuid
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file || busy.value)
    return
  message.value = ''
  if (file.size > 16 * 1024) {
    message.value = '回程报告不能超过 16 KiB'
    input.value = ''
    return
  }
  busy.value = true
  try {
    const report: unknown = JSON.parse(await file.text())
    const result = await getSharedApi().post<{ data: ReturnRoute[] }>(`admin/nodes/${encodeURIComponent(uuid)}/return-routes`, report)
    const current = nodesStore.nodes.find(node => node.uuid === uuid)
    if (current)
      current.return_routes = result.data
    message.value = '上海 TCP 回程报告已保存。'
  }
  catch (error) {
    message.value = error instanceof Error
      ? error.message
      : '报告导入失败'
  }
  finally {
    busy.value = false
    input.value = ''
  }
}
</script>

<template>
  <details v-if="node.return_routes?.length || node.ipv4 || node.ipv6" class="text-xs" @click.stop @keydown.stop>
    <summary class="min-h-8 cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
      <span v-if="!node.return_routes?.length" class="text-muted-foreground">上海 TCP 回程 · 未检测</span>
      <span v-for="report in node.return_routes" :key="report.family" class="flex flex-wrap items-center gap-1.5 py-1">
        <span class="text-muted-foreground">V{{ report.family }} 上海</span>
        <span v-if="report.address_changed" class="text-muted-foreground">地址已变，需重新检测</span>
        <span v-for="route in report.carriers" :key="route.carrier" class="whitespace-nowrap rounded-full bg-muted px-2 py-1" :class="{ 'text-muted-foreground': report.stale || route.status !== 'ok' }">
          {{ names[route.carrier] ?? route.carrier }} {{ route.status === 'ok' ? route.label : '未知' }}
        </span>
        <span v-if="report.stale" class="text-muted-foreground">已过期</span>
      </span>
    </summary>
    <div class="mt-2 space-y-2 rounded-md border border-border p-3">
      <p>仅代表上海三网 TCP 检测结果，不能代表其他地区、ICMP 或全部业务路径；七天后标记过期。</p>
      <div v-for="report in node.return_routes" :key="report.family">
        <p>IPv{{ report.family }} · {{ report.source }} · {{ new Date(report.checked_at_unix_ms).toLocaleString() }}</p>
        <p v-for="route in report.carriers" :key="route.carrier" class="break-all">{{ names[route.carrier] }}：{{ route.target }} · {{ route.label }}</p>
      </div>
      <label v-if="node.ipv4 || node.ipv6" class="grid min-h-11 gap-2">
        导入管理工具生成的上海回程 JSON（仅管理员）
        <input type="file" accept="application/json,.json" :disabled="busy" class="w-full min-w-0" @change="importReport">
      </label>
      <p v-if="message" role="status">{{ message }}</p>
    </div>
  </details>
</template>
