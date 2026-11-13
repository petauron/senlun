import type { NodeData } from '@/stores/nodes'
import { computed } from 'vue'
import { useAppStore } from '@/stores/app'
import { selectHomeProbes } from '@/utils/nodeHelpers'

export function useNodeIpVersion(node: () => NodeData) {
  const app = useAppStore()
  const versions = computed(() => (['ipv4', 'ipv6'] as const).filter((version) => {
    const value = node()
    const family = version === 'ipv4' ? 4 : 6
    return Boolean(value[version])
      || value.ip_scores?.some(entry => entry.family === family)
      || value.return_routes?.some(report => report.family === family)
      || selectHomeProbes(value.probes ?? [], value.uuid, app.publicSettings?.theme_settings, version).some(probe => probe !== null)
  }))
  const version = computed<'ipv4' | 'ipv6'>({
    get: () => {
      const preferred = app.getHomeProbeIPVersion(node().uuid)
      return versions.value.includes(preferred)
        ? preferred
        : versions.value[0] ?? 'ipv4'
    },
    set: value => app.setHomeProbeIPVersion(node().uuid, value),
  })
  const family = computed<4 | 6>(() => version.value === 'ipv4' ? 4 : 6)
  return { versions, version, family }
}
