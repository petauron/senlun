<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { Button } from '@/components/ui/button'
import { useNodeIpVersion } from '@/composables/useNodeIpVersion'

const props = withDefaults(defineProps<{ node: NodeData, mobileTouchTarget?: boolean }>(), {
  mobileTouchTarget: false,
})
const { versions, version } = useNodeIpVersion(() => props.node)
</script>

<template>
  <div
    v-if="versions.length > 1"
    role="group"
    :aria-label="`${node.name} IP 版本`"
    class="relative isolate inline-grid grid-cols-2 items-center rounded-full bg-muted/60"
    :class="mobileTouchTarget ? 'p-1 sm:p-0.5' : 'p-0.5'"
  >
    <span
      aria-hidden="true"
      class="pointer-events-none absolute -z-10"
      :class="mobileTouchTarget ? 'inset-1 sm:inset-0.5' : 'inset-0.5'"
    >
      <span
        class="block h-full w-1/2 rounded-full bg-background shadow-sm motion-safe:transition-transform motion-safe:duration-100 motion-safe:ease-out"
        :class="{ 'translate-x-full': version === 'ipv6' }"
      />
    </span>
    <Button
      v-for="item in versions" :key="item" variant="ghost" size="xs"
      class="rounded-full transition-none active:not-aria-[haspopup]:translate-y-0"
      :aria-pressed="version === item"
      :aria-label="`${node.name} 显示 ${item === 'ipv4' ? 'IPv4' : 'IPv6'}`"
      :class="[
        mobileTouchTarget
          ? 'h-auto min-h-11 min-w-14 px-3.5 text-xs sm:h-6 sm:min-h-6 sm:min-w-0 sm:px-2.5 sm:text-[10px]'
          : 'min-h-6 px-2.5 text-[10px]',
        version === item
          ? 'text-emerald-700 hover:bg-transparent hover:text-emerald-700 dark:text-emerald-300 dark:hover:bg-transparent dark:hover:text-emerald-300'
          : mobileTouchTarget
            ? 'text-foreground/75 sm:text-muted-foreground'
            : 'text-muted-foreground',
      ]"
      @click.stop="version = item" @keydown.enter.stop @keydown.space.stop
    >
      {{ item === 'ipv4' ? 'IPv4' : 'IPv6' }}
    </Button>
  </div>
</template>
