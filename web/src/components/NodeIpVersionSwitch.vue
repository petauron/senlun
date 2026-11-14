<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { Button } from '@/components/ui/button'
import { useNodeIpVersion } from '@/composables/useNodeIpVersion'

const props = defineProps<{ node: NodeData }>()
const { versions, version } = useNodeIpVersion(() => props.node)
</script>

<template>
  <div v-if="versions.length > 1" role="group" :aria-label="`${node.name} IP 版本`" class="relative isolate inline-grid grid-cols-2 items-center rounded-full bg-muted/60 p-0.5" @click.stop @keydown.enter.stop @keydown.space.stop>
    <span aria-hidden="true" class="pointer-events-none absolute inset-0.5 -z-10">
      <span
        class="block h-full w-1/2 rounded-full bg-background shadow-sm motion-safe:transition-transform motion-safe:duration-100 motion-safe:ease-out"
        :class="{ 'translate-x-full': version === 'ipv6' }"
      />
    </span>
    <Button
      v-for="item in versions" :key="item" variant="ghost" size="xs"
      class="min-h-6 rounded-full px-2.5 text-[10px] transition-none active:not-aria-[haspopup]:translate-y-0"
      :aria-pressed="version === item"
      :aria-label="`${node.name} 显示 ${item === 'ipv4' ? 'IPv4' : 'IPv6'}`"
      :class="version === item ? 'text-emerald-700 hover:bg-transparent hover:text-emerald-700 dark:text-emerald-300 dark:hover:bg-transparent dark:hover:text-emerald-300' : 'text-muted-foreground'"
      @click="version = item"
    >
      {{ item === 'ipv4' ? 'IPv4' : 'IPv6' }}
    </Button>
  </div>
</template>
