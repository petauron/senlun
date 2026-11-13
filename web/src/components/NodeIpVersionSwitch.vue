<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { Button } from '@/components/ui/button'
import { useNodeIpVersion } from '@/composables/useNodeIpVersion'

const props = defineProps<{ node: NodeData }>()
const { versions, version } = useNodeIpVersion(() => props.node)
</script>

<template>
  <div v-if="versions.length > 1" role="group" :aria-label="`${node.name} IP 版本`" class="inline-flex items-center gap-0.5 rounded-full bg-muted/60 p-0.5" @click.stop @keydown.enter.stop @keydown.space.stop>
    <Button
      v-for="item in versions" :key="item" variant="ghost" size="xs"
      class="min-h-6 rounded-full px-2.5 text-[10px] transition-colors"
      :aria-pressed="version === item"
      :aria-label="`${node.name} 显示 ${item === 'ipv4' ? 'IPv4' : 'IPv6'}`"
      :class="version === item ? 'bg-background text-emerald-700 shadow-sm dark:text-emerald-300' : 'text-muted-foreground'"
      @click="version = item"
    >
      {{ item === 'ipv4' ? 'IPv4' : 'IPv6' }}
    </Button>
  </div>
</template>
