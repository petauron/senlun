<script setup lang="ts">
import type { NodeData } from '@/stores/nodes'
import { computed } from 'vue'
import { getBandwidthBadge, nodeBadgeClass } from '@/utils/nodeBadges'

const props = defineProps<{ node: NodeData }>()
const bandwidth = computed(() => getBandwidthBadge(props.node.tags))
</script>

<template>
  <span
    v-if="bandwidth"
    :class="[nodeBadgeClass, bandwidth.tone]"
    :title="`标称带宽 ${bandwidth.text}`"
    :aria-label="`标称带宽 ${bandwidth.text}`"
  >{{ bandwidth.text }}</span>
</template>
