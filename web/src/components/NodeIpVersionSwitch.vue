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
    class="relative isolate inline-grid grid-cols-2 items-center rounded-full"
    :class="mobileTouchTarget ? 'bg-transparent p-0 sm:bg-muted/60 sm:p-0.5' : 'bg-muted/60 p-0.5'"
  >
    <span
      aria-hidden="true"
      data-testid="ip-version-track"
      class="pointer-events-none absolute -z-10"
      :class="mobileTouchTarget
        ? 'inset-x-0 top-1/2 h-[26px] -translate-y-1/2 rounded-full bg-muted/60 p-0.5 sm:inset-0.5 sm:top-0.5 sm:h-auto sm:translate-y-0 sm:bg-transparent sm:p-0'
        : 'inset-0.5'"
    >
      <span
        data-testid="ip-version-indicator"
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
          ? 'group/ip-option h-11 min-h-11 w-11 min-w-11 bg-transparent px-0.5 hover:bg-transparent hover:text-inherit dark:hover:bg-transparent sm:h-6 sm:min-h-6 sm:w-auto sm:min-w-0 sm:px-0'
          : 'min-h-6 px-2.5 text-[10px]',
        !mobileTouchTarget && (
          version === item
            ? 'text-emerald-700 hover:bg-transparent hover:text-emerald-700 dark:text-emerald-300 dark:hover:bg-transparent dark:hover:text-emerald-300'
            : 'text-muted-foreground'
        ),
      ]"
      @click.stop="version = item" @keydown.enter.stop @keydown.space.stop
    >
      <span
        v-if="mobileTouchTarget"
        class="inline-flex h-[22px] w-full items-center justify-center rounded-full px-1 text-[11px] leading-none transition-colors motion-reduce:transition-none sm:h-full sm:px-2.5 sm:text-[10px]"
        :class="version === item
          ? 'font-semibold text-emerald-700 dark:text-emerald-300'
          : 'text-foreground/75 group-hover/ip-option:bg-background/55 group-hover/ip-option:text-foreground dark:group-hover/ip-option:bg-background/35 dark:group-hover/ip-option:text-foreground'"
      >
        {{ item === 'ipv4' ? 'IPv4' : 'IPv6' }}
      </span>
      <template v-else>
        {{ item === 'ipv4' ? 'IPv4' : 'IPv6' }}
      </template>
    </Button>
  </div>
</template>
