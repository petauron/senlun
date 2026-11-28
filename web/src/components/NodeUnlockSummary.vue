<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { computed } from 'vue'
import { CardX } from '@/components/ui/card-x'
import { useBackgroundSurface } from '@/composables/useBackgroundSurface'
import { useNodeIpVersion } from '@/composables/useNodeIpVersion'
import { getGoogleUnlockDisplay, getOrderedUnlockServices, getUnlockPanelState } from '@/utils/ipUnlock'
import { nodeBadgeClass } from '@/utils/nodeBadges'

const props = defineProps<{ node: NodeData, mode: 'badge' | 'panel' }>()
const { pickSurfaceClass } = useBackgroundSurface()
const { family } = useNodeIpVersion(() => props.node)
const report = computed(() => props.node.ip_unlocks?.find(item => item.family === family.value))
const active = computed(() => Boolean(report.value && !report.value.address_changed))
const googleInfo = computed(() => getGoogleUnlockDisplay(report.value))
const panelState = computed(() => getUnlockPanelState(report.value, family.value))
const services = computed(() => getOrderedUnlockServices(report.value, family.value))
</script>

<template>
  <span
    v-if="mode === 'badge' && active"
    :class="[nodeBadgeClass, googleInfo.tone]"
    :title="googleInfo.hint || googleInfo.label"
  >
    Google {{ googleInfo.label }}
  </span>
  <div v-else-if="mode === 'panel'" class="px-4">
    <CardX
      size="small"
      class="border-none rounded-md transition-all"
      :class="pickSurfaceClass('bg-background/60', 'bg-background/50 backdrop-blur-xs')"
      content-class="!p-3"
      data-testid="unlock-summary-panel"
    >
      <div class="space-y-2.5">
        <div class="flex flex-wrap items-center justify-between gap-2 text-xs">
          <span class="font-semibold text-foreground">流媒体与 AI 解锁 · IPv{{ family }}</span>
          <span v-if="active && googleInfo.hint" class="text-[11px] text-muted-foreground">{{ googleInfo.hint }}</span>
          <span
            v-else-if="!active"
            class="shrink-0 rounded-full border px-1.5 py-0.5 text-[10px] font-semibold leading-none"
            :class="panelState.badgeTone"
          >
            {{ panelState.badgeLabel }}
          </span>
        </div>
        <div
          v-if="active"
          class="grid grid-cols-2 gap-1.5 sm:grid-cols-3"
          :class="services.length === 5 ? 'lg:grid-cols-5' : 'lg:grid-cols-6'"
        >
          <div
            v-for="item in services"
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
        <div
          v-else
          class="rounded-lg border border-border/60 bg-muted/35 px-3 py-2 text-xs"
          :data-unlock-state="panelState.status"
        >
          <p class="font-medium text-foreground">
            {{ panelState.title }}
          </p>
          <p class="mt-0.5 text-[11px] text-foreground/75">
            {{ panelState.description }}
          </p>
        </div>
      </div>
    </CardX>
  </div>
</template>
