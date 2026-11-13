<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { computed, ref } from 'vue'
import { getSharedApi } from '@/utils/api'

const props = defineProps<{ node: NodeData }>()
const busy = ref(false)
const message = ref('')
const scores = computed(() => props.node.ip_scores ?? [])
async function refresh(family: 4 | 6) {
  const ip = family === 4 ? props.node.ipv4 : props.node.ipv6
  if (!ip || busy.value)
    return
  busy.value = true
  message.value = ''
  try {
    const result = await getSharedApi().post<{ data: { reputation: { trust_score: number | null } } }>('admin/ip-info/v1/refresh', { uuid: props.node.uuid, ip })
    const score = result.data.reputation.trust_score
    message.value = score === null ? `IPv${family}：来源未提供评分` : `IPv${family} Net.Coffee 评分 ${score}，已更新`
  }
  catch (error) {
    message.value = error instanceof Error ? error.message : '评分更新失败'
  }
  finally {
    busy.value = false
  }
}
</script>

<template>
  <details v-if="scores.length" class="text-xs" @click.stop @keydown.stop>
    <summary class="min-h-8 cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
      <span class="inline-flex flex-wrap gap-2">
        <span v-for="entry in scores" :key="entry.family" class="whitespace-nowrap rounded-full bg-muted px-2 py-1" :class="{ 'text-muted-foreground': entry.score === null || entry.stale }">
          V{{ entry.family }} IP {{ entry.score ?? '—' }}{{ entry.stale ? ' · 已过期' : '' }}
        </span>
      </span>
    </summary>
    <div class="mt-2 space-y-2 rounded-md border border-border p-3">
      <p>Net.Coffee 原始评分（0–100，越高越好）；不是滥用概率或可用性保证。</p>
      <div v-for="entry in scores" :key="entry.family" class="flex flex-wrap items-center justify-between gap-2">
        <span>IPv{{ entry.family }} · {{ entry.updated_at_unix_ms ? new Date(entry.updated_at_unix_ms).toLocaleString() : '尚未查询' }}</span>
        <button type="button" class="min-h-11 rounded-md border border-border px-3 focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50" :disabled="busy" @click.stop="refresh(entry.family)">
          {{ busy ? '查询中…' : '更新评分' }}
        </button>
      </div>
      <p v-if="message" role="status">{{ message }}</p>
    </div>
  </details>
</template>
