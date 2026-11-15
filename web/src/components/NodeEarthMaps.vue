<script setup lang="ts">
import type { EChartsOption } from 'echarts/types/dist/shared'
import type { NodeData } from '@/stores/nodes'
import { computed, watch, ref } from 'vue'
import { useRegionTraffic } from '@/composables/useRegionTraffic'
import { useAppStore } from '@/stores/app'
import VChart from 'vue-echarts'
import { Empty } from '@/components/ui/empty'
import { Spinner } from '@/components/ui/spinner'
import { useNodesStore } from '@/stores/nodes'
import { ensureWorldMapRegistered } from '@/utils/echartsWorldMap'
import { getCoordByCode, getCountryCodeFromRegion } from '@/utils/geoHelper'
import { getRegionDisplayName } from '@/utils/regionHelper'
import '@/utils/echarts'

interface EarthMapPoint {
  code: string
  name: string
  coord: [number, number]
  online: number
  total: number
}

const props = defineProps<{
  desktop: boolean
  nodes?: NodeData[]
}>()

const nodesStore = useNodesStore()
const displayNodes = computed(() => props.nodes ?? nodesStore.earthNodes)
const { rateFor, formatRate } = useRegionTraffic(displayNodes)
const selected = ref('')
function selectRegion(code: string) {
  if (rateFor(code).nodes.length) selected.value = selected.value === code ? '' : code
}
const detail = computed(() => points.value.find((p) => p.code === selected.value))
const detailNodes = computed(() =>
  [...rateFor(selected.value).nodes].sort(
    (a, b) => Number(b.online) - Number(a.online) || b.net_out + b.net_in - (a.net_out + a.net_in),
  ),
)
const mapName = ref<string>()
const loading = ref(true)
const loadError = ref<string | null>(null)

const regionDisplayNames = typeof Intl.DisplayNames === 'function' ? new Intl.DisplayNames(['zh-Hans'], { type: 'region' }) : null

function resolveCountryDisplayName(region: string, code: string): string {
  const regionName = getRegionDisplayName(region)
  if (regionName !== region) return regionName

  return regionDisplayNames?.of(code) ?? code
}

const points = computed<EarthMapPoint[]>(() => {
  const map = new Map<string, EarthMapPoint>()
  for (const node of displayNodes.value) {
    const code = getCountryCodeFromRegion(node.region)
    if (!code) continue
    const coord = getCoordByCode(code)
    if (!coord) continue
    const current = map.get(code)
    if (!current) {
      map.set(code, {
        code,
        name: resolveCountryDisplayName(node.region, code),
        coord,
        online: node.online ? 1 : 0,
        total: 1,
      })
      continue
    }
    current.total += 1
    current.online += node.online ? 1 : 0
  }

  return Array.from(map.values()).sort((a, b) => b.online - a.online || b.total - a.total)
})

const totalServers = computed(() => displayNodes.value.length)
const onlineServers = computed(() => displayNodes.value.filter((node) => node.online).length)
const offlineServers = computed(() => totalServers.value - onlineServers.value)

const app = useAppStore()
const shade = (light: string, dark: string) => (app.isDark ? dark : light)
const bands = computed(() => [
  { min: 0, label: '0', color: shade('#b4c2c6', '#586772') },
  { min: 1, label: '<32 KB/s', color: shade('#a6d6ca', '#355e63') },
  { min: 32 * 1024, label: '32–256 KB/s', color: shade('#69b9a5', '#338980') },
  { min: 256 * 1024, label: '256 KB/s–1 MB/s', color: shade('#318e7b', '#51b89d') },
  { min: 1024 * 1024, label: '≥1 MB/s', color: shade('#136554', '#99e4be') },
])
const mapSeriesData = computed(() =>
  points.value.map((point) => {
    const { up, down } = rateFor(point.code)
    const value = up + down
    const color = bands.value.filter((b) => value >= b.min).at(-1)?.color ?? bands.value[0]!.color
    return {
      name: point.code,
      value,
      coord: [point.coord[1], point.coord[0]] as [number, number],
      itemStyle: { areaColor: color, color },
      emphasis: { itemStyle: { areaColor: color, borderColor: shade('#245d50', '#d9f1e8') } },
    }
  }),
)
const callouts = {
  HK: ['香港', 'left'],
  TW: ['台湾', 'right'],
  JP: ['日本', 'top'],
  SG: ['新加坡', 'bottom'],
} as const
const chartOption = computed<EChartsOption>(() => ({
  animationDurationUpdate: 300,
  animationEasingUpdate: 'cubicOut',
  series: [
    {
      type: 'map',
      map: mapName.value,
      roam: false,
      selectedMode: false,
      left: 'center',
      top: 'middle',
      height: '90%',
      aspectScale: 1,
      emphasis: {
        label: { show: false },
        itemStyle: {
          areaColor: shade('#e3eaec', '#394755'),
          borderColor: shade('#73968d', '#a3c4c2'),
          borderWidth: 0.5,
        },
      },
      itemStyle: {
        areaColor: shade('#e3eaec', '#394755'),
        borderColor: shade('#bdcbd0', '#26313c'),
        borderWidth: 0.5,
      },
      data: mapSeriesData.value,
      markPoint: {
        symbol: 'circle',
        symbolSize: 5,
        label: { show: false },
        data: mapSeriesData.value.map((r) => {
          const callout = callouts[r.name as keyof typeof callouts]
          return {
            name: r.name,
            coord: r.coord,
            value: r.value,
            symbolSize: callout ? 7 : 5,
            itemStyle: { color: r.itemStyle.color, borderColor: shade('#ffffff', '#e0f4ec'), borderWidth: 1 },
            label: callout
              ? {
                  show: true,
                  formatter: callout[0],
                  position: callout[1],
                  distance: 3,
                  fontSize: 12,
                  fontWeight: 500,
                  color: shade('#23463e', '#f0f7f5'),
                  backgroundColor: shade('#f3faf7', '#14212a'),
                  padding: [4, 5] as [number, number],
                  borderRadius: 3,
                }
              : { show: false },
          }
        }),
      },
      label: { show: false },
    },
  ],
}))

watch(
  () => props.desktop,
  async (visible) => {
    if (!visible || mapName.value) return
    loading.value = true
    loadError.value = null

    try {
      mapName.value = await ensureWorldMapRegistered()
    } catch (error) {
      loadError.value = error instanceof Error ? error.message : '地图资源加载失败'
    } finally {
      loading.value = false
    }
  },
  { immediate: true },
)
</script>

<template>
  <div
    class="relative h-full border-none"
    content-class="h-full !p-0"
    data-testid="earth-map"
    role="group"
    :aria-label="`世界地图：共 ${totalServers} 个节点，在线 ${onlineServers} 个，离线 ${offlineServers} 个`"
  >
    <div class="flat-surface">
      <div v-if="desktop" class="geo">
        <div class="flat-chart">
          <Spinner :show="loading" class="h-full w-full" content-class="!bg-transparent">
            <VChart v-if="mapName" :option="chartOption" @click="selectRegion($event.name)" autoresize class="h-full w-full" />
            <Empty v-else-if="loadError" description="地图资源加载失败" class="h-full" />
          </Spinner>
        </div>
        <div class="legend">
          <b>收发合计</b
          ><span v-for="band in bands" :key="band.min"><i :style="{ background: band.color }"></i>{{ band.label }}</span>
        </div>
      </div>
      <div class="region-counts" aria-label="地区上下行速率">
        <h3 v-if="desktop">地区流量</h3>
        <div class="columns headings"><span>地区</span><span>在线</span><span>↑ 上行</span><span>↓ 下行</span></div>
        <button
          class="columns"
          v-for="p in points"
          :key="p.code"
          :data-code="p.code"
          :aria-expanded="selected === p.code"
          @click="selectRegion(p.code)"
        >
          <span class="place">{{ p.name }}</span
          ><span>{{ p.online }}/{{ p.total }}</span> <span class="up">{{ formatRate(rateFor(p.code).up) }}</span
          ><span class="down">{{ formatRate(rateFor(p.code).down) }}</span>
        </button>
      </div>
      <section v-if="detail" class="traffic-detail" aria-label="地区节点速率">
        <header>
          <strong>{{ detail.name }} · 节点速率</strong><button aria-label="关闭地区详情" @click="selected = ''">收起</button>
        </header>
        <table>
          <thead>
            <tr>
              <th>节点</th>
              <th>↑ 上行</th>
              <th>↓ 下行</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="n in detailNodes" :key="n.uuid">
              <th>{{ n.name }}</th>
              <td>{{ n.online ? formatRate(n.net_out || 0) : '离线' }}</td>
              <td>{{ n.online ? formatRate(n.net_in || 0) : '—' }}</td>
            </tr>
          </tbody>
        </table>
      </section>
    </div>
  </div>
</template>
<style scoped>
.flat-surface {
  display: grid;
  grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr);
  align-items: center;
  gap: 8px 36px;
  border-top: 1px solid #ffffff16;
  padding-top: 10px;
}
.geo {
  position: relative;
  min-width: 0;
}
.legend {
  display: flex;
  justify-content: center;
  flex-wrap: wrap;
  gap: 3px 10px;
  color: #adbac5;
  font-size: 10px;
}
.legend b {
  font-weight: 550;
}
.legend span {
  display: inline-flex;
  align-items: center;
  gap: 3px;
}
.legend i {
  display: inline-block;
  width: 9px;
  height: 7px;
  border-radius: 2px;
}
.flat-chart {
  width: 100%;
  aspect-ratio: 2.6;
  height: auto;
  max-height: 240px;
}
.geo :deep(.aspect-square) {
  width: 192px;
  translate: none;
  transform: none;
}
.region-counts h3 {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 6px;
}
.columns {
  width: 100%;
  display: grid;
  grid-template-columns: 1.25fr 0.7fr 1fr 1fr;
  text-align: right;
  align-items: center;
  gap: 4px;
  font-variant-numeric: tabular-nums;
}
.columns :first-child {
  text-align: left;
}
.headings {
  font-size: 11px;
  color: #a8b4c2;
}
.headings :nth-child(3) {
  color: #96bfa7;
}
.headings :last-child {
  color: #9cbdd8;
}
.region-counts button {
  padding: 5px 0;
  border-bottom: 1px solid #ffffff0e;
  font-size: 12px;
  cursor: pointer;
}
.region-counts button:hover,
.region-counts button[aria-expanded='true'] {
  background: #ffffff09;
}
.traffic-detail {
  grid-column: 1/-1;
}
.traffic-detail {
  margin-top: 8px;
  padding: 10px;
  background: #141d25;
  border-radius: 8px;
  font-size: 12px;
}
.traffic-detail header {
  display: flex;
  justify-content: space-between;
  margin-bottom: 5px;
}
.traffic-detail button {
  color: #afc1cd;
  cursor: pointer;
}
.traffic-detail table {
  width: 100%;
  font-variant-numeric: tabular-nums;
}
.traffic-detail th,
.traffic-detail td {
  padding: 3px;
  text-align: right;
  font-weight: 400;
}
.traffic-detail th:first-child {
  text-align: left;
}
.traffic-detail thead {
  color: #aab8c3;
}
@media (max-width: 640px) {
  .flat-surface {
    display: block;
    border: none;
    padding: 0 0 10px;
  }
  .region-counts button {
    padding: 7px 0;
  }
  .region-counts .columns {
    font-size: 11px;
  }
  .traffic-detail {
    margin-bottom: 4px;
  }
}

.flat-surface {
  --map-up: #316c50;
  --map-down: #346585;
  border-color: var(--border);
}
:is(.dark .flat-surface) {
  --map-up: #96bfa7;
  --map-down: #9cbdd8;
}
.legend,
.headings,
.traffic-detail thead,
.traffic-detail button {
  color: var(--muted-foreground);
}
.region-counts button {
  border-color: var(--border);
}
.region-counts button:is(:hover, [aria-expanded='true']) {
  background: var(--accent);
}
.traffic-detail {
  background: var(--muted);
  color: var(--foreground);
}
.up,
.headings > :nth-child(3),
.traffic-detail :is(td, th):nth-child(2) {
  color: var(--map-up);
}
.down,
.headings > :last-child,
.traffic-detail :is(td, th):nth-child(3) {
  color: var(--map-down);
}
</style>
