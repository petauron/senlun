import { computed, type Ref } from 'vue'
import type { NodeData } from '@/stores/nodes'
import { useAppStore } from '@/stores/app'
import { getCountryCodeFromRegion } from '@/utils/geoHelper'
import { formatBytesPerSecondSplit } from '@/utils/helper'

export function useRegionTraffic(nodes: Ref<NodeData[]>) {
  const app = useAppStore()
  const regions = computed(() => {
    const groups = new Map<string, { up: number; down: number; nodes: NodeData[] }>()
    for (const node of nodes.value) {
      const code = getCountryCodeFromRegion(node.region)
      if (!code) continue
      const group = groups.get(code) ?? { up: 0, down: 0, nodes: [] }
      group.nodes.push(node)
      if (node.online) {
        group.up += node.net_out || 0
        group.down += node.net_in || 0
      }
      groups.set(code, group)
    }
    return groups
  })
  function rateFor(code: string) {
    return regions.value.get(code) ?? { up: 0, down: 0, nodes: [] }
  }
  function formatRate(bytes: number) {
    const { value, unit } = formatBytesPerSecondSplit(bytes, app.byteDecimals)
    return value + ' ' + unit
  }
  return { rateFor, formatRate }
}
