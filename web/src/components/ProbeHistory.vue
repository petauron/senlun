<script setup lang="ts">
import type { EChartsOption } from 'echarts'
import type { ProbeHistory } from '@/utils/admin'
import { Icon } from '@iconify/vue'
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import VChart from 'vue-echarts'
import { Button } from '@/components/ui/button'
import { DataTooltip } from '@/components/ui/data-tooltip'
import { Empty } from '@/components/ui/empty'
import SelectControl from '@/components/ui/SelectControl.vue'
import { Spinner } from '@/components/ui/spinner'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useBackgroundSurface } from '@/composables/useBackgroundSurface'
import { useAppStore } from '@/stores/app'
import { fetchProbeHistory } from '@/utils/admin'
import { probeChartSeries, probeFailureSeries } from '@/utils/probeChart'
import { failureRateLabel, failureRateLabelForKinds } from '@/utils/probeLabels'
import '@/utils/echarts'

const props = defineProps<{ uuid: string, kind?: 'icmp' | 'tcp' | 'http', taskId?: string, selectedHours?: number }>()
const app = useAppStore()
const { pickSurfaceClass } = useBackgroundSurface()
const presetRanges = [1, 6, 12, 24, 72, 168]
const configuredHours = app.publicSettings?.theme_settings?.pingChartDefaultHours
const defaultHours = typeof configuredHours === 'number' && presetRanges.includes(configuredHours)
  ? configuredHours
  : 1
const hours = ref(props.selectedHours ?? defaultHours)
const chartMetric = ref<'latency' | 'failure' | 'combined'>('latency')
const showDelay = computed(() => chartMetric.value !== 'failure')
const showFailures = computed(() => chartMetric.value !== 'latency')
const showThresholds = ref(false)
const showPeaks = ref(false)
const cutPeak = ref(false)
const chartLayout = ref<'combined' | 'split'>(app.publicSettings?.theme_settings?.pingChartLayout === 'split'
  ? 'split'
  : 'combined')
const autoRefresh = ref(app.publicSettings?.theme_settings?.pingChartAutoRefresh !== false)
const combinedChartRef = ref<unknown>(null)
const latencyChartRef = ref<unknown>(null)
const failureChartRef = ref<unknown>(null)
const exportError = ref('')
const filenameUnsafeChars = /[^\w-]/g
const spreadsheetFormulaPrefix = /^\s*[-=+@]/
const availableRanges = computed(() => {
  const max = app.publicSettings?.ping_record_preserve_time ?? 168
  const presets = presetRanges.filter(range => range <= max)
  if (max > 1 && !presets.includes(max))
    presets.push(max)
  return presets
})
const refreshIntervalSeconds = computed(() => {
  const value = app.publicSettings?.theme_settings?.pingChartRefreshInterval
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.round(Math.min(300, Math.max(10, value)))
    : 60
})
let refreshTimer: ReturnType<typeof setInterval> | undefined
const data = shallowRef<ProbeHistory | null>(null)
const loading = ref(false)
const error = ref('')
let controller: AbortController | null = null
let generation = 0

async function reload(): Promise<void> {
  controller?.abort()
  const current = ++generation
  controller = new AbortController()
  loading.value = true
  error.value = ''
  try {
    const response = await fetchProbeHistory(props.uuid, hours.value, props.taskId, controller.signal)
    if (current === generation)
      data.value = response
  }
  catch (cause) {
    if (current === generation) {
      data.value = null
      error.value = cause instanceof Error
        ? cause.message
        : String(cause)
    }
  }
  finally {
    if (current === generation)
      loading.value = false
  }
}

function toggleAutoRefresh(): void {
  autoRefresh.value = !autoRefresh.value
}

watch([autoRefresh, refreshIntervalSeconds], ([enabled, seconds]) => {
  clearInterval(refreshTimer)
  refreshTimer = undefined
  if (enabled) {
    refreshTimer = setInterval(() => {
      if (document.visibilityState === 'visible' && !loading.value)
        void reload()
    }, seconds * 1000)
  }
}, { immediate: true })

watch(() => props.selectedHours, (value) => {
  if (value !== undefined)
    hours.value = value
})
watch(availableRanges, (available) => {
  if (props.selectedHours === undefined && !available.includes(hours.value))
    hours.value = available[0] ?? 1
}, { immediate: true })
watch(() => [props.uuid, props.taskId, hours.value], () => {
  data.value = null
  void reload()
}, { immediate: true })
onBeforeUnmount(() => {
  clearInterval(refreshTimer)
  ++generation
  controller?.abort()
})
const tasks = computed(() => (data.value?.tasks ?? []).filter(task => (!props.kind || task.kind === props.kind) && (!props.taskId || task.id === props.taskId)))
const taskIds = computed(() => new Set(tasks.value.map(task => task.id)))
const selectedTaskIds = ref<string[]>([])
const analysisTaskId = ref('')
let selectionInitialized = false
watch(tasks, (available) => {
  const ids = available.map(task => task.id)
  if (!ids.includes(analysisTaskId.value))
    analysisTaskId.value = ids[0] ?? ''
  if (!ids.length)
    return
  if (!selectionInitialized && ids.length) {
    selectedTaskIds.value = ids
    selectionInitialized = true
  }
  else if (selectionInitialized) {
    selectedTaskIds.value = selectedTaskIds.value.filter(id => ids.includes(id))
  }
})
watch(() => [props.uuid, props.taskId, props.kind], () => {
  selectionInitialized = false
  selectedTaskIds.value = []
  analysisTaskId.value = ''
})
const selectedTasks = computed(() => tasks.value.filter(task => selectedTaskIds.value.includes(task.id)))
const summaryByTask = computed(() => new Map(data.value?.summary.map(summary => [summary.task_id, summary]) ?? []))

const lineSelection = computed({
  get: () => selectedTaskIds.value.length === tasks.value.length
    ? 'all'
    : selectedTaskIds.value.length === 1
      ? selectedTaskIds.value[0]!
      : 'custom',
  set: (value: string) => {
    if (value === 'all') {
      selectedTaskIds.value = tasks.value.map(task => task.id)
    }
    else if (taskIds.value.has(value)) {
      selectedTaskIds.value = [value]
      analysisTaskId.value = value
    }
  },
})
const lineOptions = computed(() => [
  { value: 'all', label: '全部线路' },
  ...(lineSelection.value === 'custom'
    ? [
        { value: 'custom', label: `已选 ${selectedTaskIds.value.length} 条` },
      ]
    : []),
  ...tasks.value.map(task => ({ value: task.id, label: task.name })),
])
const chartLatencyLabel = computed(() => tasks.value.some(task => task.kind === 'http')
  ? '延迟'
  : 'RTT')

const analysisTask = computed(() => tasks.value.find(task => task.id === analysisTaskId.value))
const analysisSummary = computed(() => summaryByTask.value.get(analysisTaskId.value))
const names = computed(() => new Map(data.value?.tasks.map(task => [task.id, task.name]) ?? []))
const recent = computed(() => [
  ...(data.value?.records ?? []),
].filter(record => taskIds.value.has(record.task_id)).sort((a, b) => b.received_at_unix_ms - a.received_at_unix_ms).slice(0, 100))
const date = (value: number) => new Date(value).toLocaleString()
function latency(value: number | null | undefined) {
  return value == null
    ? '无成功样本'
    : `${value.toFixed(1)} ms`
}
function cardLatency(value: number | null | undefined) {
  return value == null
    ? '--'
    : `${value.toFixed(2)} ms`
}
function analysisLatency(value: number | null | undefined) {
  return value == null
    ? '--'
    : `${Math.round(value)} ms`
}
const colors = [
  '#FF6B6B',
  '#F59E0B',
  '#10B981',
  '#22D3EE',
  '#60A5FA',
  '#A78BFA',
  '#34D399',
  '#FB923C',
]
const hexColor = /^#[0-9a-f]{6}$/i
function taskColor(id: string): string {
  const configured = app.publicSettings?.theme_settings?.pingTaskColors
  if (configured && typeof configured === 'object' && !Array.isArray(configured)) {
    const color = (configured as Record<string, unknown>)[id]
    if (typeof color === 'string' && hexColor.test(color))
      return color.toUpperCase()
  }
  return colors[tasks.value.findIndex(task => task.id === id) % colors.length] ?? colors[0]!
}
function rangeLabel(range: number) {
  return range < 24 || range % 24 !== 0
    ? `${range} 小时`
    : `${range / 24} 天`
}
function tooltipTime(value: number): string {
  const date = new Date(value)
  const pad = (number: number) => String(number).padStart(2, '0')
  const clock = `${pad(date.getHours())}:${pad(date.getMinutes())}`
  return hours.value < 24
    ? `${clock}:${pad(date.getSeconds())}`
    : `${pad(date.getMonth() + 1)}/${pad(date.getDate())} ${clock}`
}

function chartTooltip(params: unknown): HTMLElement {
  const root = document.createElement('div')
  const points = (Array.isArray(params)
    ? params
    : [params]) as { seriesId?: unknown, seriesName?: unknown, data?: unknown, value?: unknown, color?: unknown }[]
  const rows = points.flatMap((point) => {
    if (!point || typeof point !== 'object')
      return []
    const pair = Array.isArray(point.data)
      ? point.data
      : point.value
    if (!Array.isArray(pair) || typeof pair[0] !== 'number' || !Number.isFinite(pair[0]) || typeof pair[1] !== 'number' || !Number.isFinite(pair[1]))
      return []
    return [
      {
        time: pair[0] as number,
        value: pair[1] as number,
        name: String(point.seriesName ?? ''),
        order: tasks.value.findIndex(task => String(point.seriesId ?? '').startsWith(`${task.id}:`)),
        failureRate: typeof point.seriesId === 'string' && point.seriesId.endsWith(':failure-rate'),
        color: typeof point.color === 'string'
          ? point.color
          : colors[0]!,
      },
    ]
  }).sort((a, b) => Number(a.failureRate) - Number(b.failureRate) || a.order - b.order)
  if (!rows.length)
    return root

  const title = document.createElement('div')
  title.style.cssText = 'font-weight:600;margin-bottom:6px'
  title.textContent = tooltipTime(rows[0]!.time)
  root.append(title)
  const list = document.createElement('div')
  list.style.cssText = 'display:flex;flex-direction:column;gap:4px'
  for (const row of rows) {
    const line = document.createElement('div')
    line.style.cssText = 'display:flex;align-items:center'
    const dot = document.createElement('span')
    dot.style.cssText = 'display:inline-block;width:8px;height:8px;border-radius:50%;margin-right:8px;flex-shrink:0'
    dot.style.backgroundColor = row.color
    const name = document.createElement('span')
    name.style.cssText = 'flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap'
    name.textContent = row.name
    const value = document.createElement('span')
    value.style.cssText = 'margin-left:16px;font-weight:600;font-variant-numeric:tabular-nums'
    value.textContent = row.failureRate
      ? `${row.value.toFixed(2)}%`
      : `${Math.round(row.value)} ms`
    line.append(dot, name, value)
    list.append(line)
  }
  root.append(list)
  return root
}
function toggleTask(id: string): void {
  selectedTaskIds.value = selectedTaskIds.value.includes(id)
    ? selectedTaskIds.value.filter(selected => selected !== id)
    : [...selectedTaskIds.value, id]
}

type ChartSeries = Array<[number, number | null]>
const seriesByTask = computed(() => {
  const result = new Map<string, { latency: ChartSeries, failure: ChartSeries }>()
  if (!data.value)
    return result
  const history = data.value
  const grouped = new Map(tasks.value.map(task => [task.id, [] as ProbeHistory['chart']]))
  for (const point of history.chart)
    grouped.get(point.task_id)?.push(point)
  for (const [id, points] of grouped) {
    result.set(id, {
      latency: probeChartSeries(points, history.range_start_unix_ms, history.range_end_unix_ms, history.chart_bucket_ms, cutPeak.value),
      failure: probeFailureSeries(points, history.range_start_unix_ms, history.range_end_unix_ms, history.chart_bucket_ms),
    })
  }
  return result
})
function taskChartData(id: string): ChartSeries {
  return seriesByTask.value.get(id)?.latency ?? []
}
function taskFailureData(id: string): ChartSeries {
  return seriesByTask.value.get(id)?.failure ?? []
}

const chartFailureLabel = computed(() => failureRateLabelForKinds(
  (selectedTasks.value.length ? selectedTasks.value : tasks.value).map(task => task.kind),
))
const mainChartLabel = computed(() => chartMetric.value === 'combined'
  ? `探测延迟和${chartFailureLabel.value}曲线，左右轴分别为毫秒和百分比，长时间缺报为断点`
  : chartMetric.value === 'latency'
    ? '独立延迟曲线，纵轴为毫秒，长时间缺报为断点'
    : `独立${chartFailureLabel.value}曲线，纵轴为百分比，长时间缺报为断点`)
const showZoom = computed(() => app.publicSettings?.theme_settings?.pingChartShowZoom !== false)
const chartThresholds = computed(() => {
  const settings = app.publicSettings?.theme_settings
  const value = (key: string, fallback: number, min: number, max: number) => {
    const candidate = settings?.[key]
    return typeof candidate === 'number' && Number.isFinite(candidate) && candidate >= min && candidate <= max
      ? candidate
      : fallback
  }
  const color = (key: string, fallback: string) => {
    const candidate = settings?.[key]
    return typeof candidate === 'string' && hexColor.test(candidate)
      ? candidate
      : fallback
  }
  return {
    latencyWarning: value('pingLatencyWarning', 160, 1, 5000),
    latencyCritical: value('pingLatencyCritical', 300, 1, 5000),
    failureWarning: value('pingLossWarning', 3, 0, 100),
    failureCritical: value('pingLossCritical', 10, 0, 100),
    warningColor: color('pingWarningColor', '#FACC15'),
    criticalColor: color('pingCriticalColor', '#F43F5E'),
  }
})

function lineStatus(id: string): { label: string, className: string } {
  const summary = summaryByTask.value.get(id)
  if (!summary?.samples)
    return { label: '无样本', className: 'bg-muted text-muted-foreground' }
  const thresholds = chartThresholds.value
  if (summary.avg_latency_ms == null || summary.avg_latency_ms >= thresholds.latencyCritical || summary.loss_percent >= thresholds.failureCritical)
    return { label: '异常', className: 'bg-rose-500/10 text-rose-700 dark:text-rose-400' }
  if (summary.avg_latency_ms >= thresholds.latencyWarning || summary.loss_percent >= thresholds.failureWarning)
    return { label: '波动', className: 'bg-amber-500/10 text-amber-700 dark:text-amber-400' }
  return { label: '正常', className: 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400' }
}

// Previews reuse the already loaded chart buckets. Keep at most 48 SVG points,
// preserve peaks, and break a preview segment whenever its bucket contains a gap.
function sparkline(points: ChartSeries): { line: string, area: string } {
  const stride = Math.max(1, Math.ceil(points.length / 48))
  const values: Array<number | null> = []
  for (let start = 0; start < points.length; start += stride) {
    const bucket = points.slice(start, start + stride).map(point => point[1])
    const observed = bucket.filter((value): value is number => value != null)
    values.push(observed.length === bucket.length
      ? Math.max(...observed)
      : null)
  }
  const present = values.filter((value): value is number => value != null)
  if (!present.length)
    return { line: '', area: '' }
  const min = Math.min(...present)
  const span = Math.max(1, Math.max(...present) - min)
  const segments: Array<Array<[number, number]>> = []
  let segment: Array<[number, number]> = []
  for (const [index, value] of values.entries()) {
    if (value == null) {
      if (segment.length)
        segments.push(segment)
      segment = []
    }
    else {
      segment.push([2 + index * 60 / Math.max(1, values.length - 1), 26 - (value - min) * 22 / span])
    }
  }
  if (segment.length)
    segments.push(segment)
  return {
    line: segments.map(part => {
      const path = `M${part.map(point => point.join(',')).join(' L')}`
      return part.length === 1
        ? `${path} l0.1,0`
        : path
    }).join(' '),
    area: segments.map(part => `M${part[0]![0]},28 L${part.map(point => point.join(',')).join(' L')} L${part.at(-1)![0]},28 Z`).join(' '),
  }
}
const sparklines = computed(() => new Map(tasks.value.map(task => [
  task.id,
  sparkline(chartMetric.value === 'failure'
    ? taskFailureData(task.id)
    : taskChartData(task.id)),
])))

function chartOption(mode: 'combined' | 'latency' | 'failure'): EChartsOption {
  const style = getComputedStyle(document.documentElement)
  const foreground = style.getPropertyValue('--foreground').trim()
  const secondary = app.isDark ? 'rgba(255,255,255,0.55)' : 'rgba(0,0,0,0.55)'
  const border = app.isDark ? 'rgba(255,255,255,0.1)' : 'rgba(0,0,0,0.06)'
  const grid = app.isDark ? 'rgba(255,255,255,0.06)' : 'rgba(0,0,0,0.06)'
  const latencySeries = showDelay.value && mode !== 'failure'
    ? selectedTasks.value.map((task, index) => ({
        id: `${task.id}:latency`,
        name: mode === 'combined'
          ? `${task.name} 延迟`
          : task.name,
        type: 'line' as const,
        yAxisIndex: 0,
        showSymbol: false,
        symbolSize: 3,
        smooth: cutPeak.value ? 0.6 : 0.1,
        lineStyle: {
          width: 1.8,
          cap: 'round' as const,
          color: taskColor(task.id),
        },
        itemStyle: {
          color: taskColor(task.id),
        },
        connectNulls: false,
        data: taskChartData(task.id),
        markLine: showThresholds.value && index === 0
          ? {
              silent: true,
              symbol: ['none', 'none'] as [string, string],
              label: { show: true, position: 'insideEndTop' as const, fontSize: 10 },
              lineStyle: { width: 1, type: 'dashed' as const, opacity: 0.65 },
              data: [
                {
                  name: '延迟警告',
                  yAxis: chartThresholds.value.latencyWarning,
                  lineStyle: { color: chartThresholds.value.warningColor },
                },
                {
                  name: '延迟严重',
                  yAxis: chartThresholds.value.latencyCritical,
                  lineStyle: { color: chartThresholds.value.criticalColor },
                },
              ],
            }
          : undefined,
        areaStyle: { color: taskColor(task.id), opacity: 0.045 },
        emphasis: { focus: 'series' as const },
        markPoint: showPeaks.value
          ? {
              symbolSize: 28,
              label: {
                fontSize: 9,
                formatter: '峰值',
              },
              data: [
                { type: 'max' as const, name: '峰值' },
              ],
            }
          : undefined,
      }))
    : []
  const failureSeries = showFailures.value && mode !== 'latency'
    ? selectedTasks.value.map((task, index) => {
        const points = taskFailureData(task.id)
        return {
          id: `${task.id}:failure-rate`,
          name: mode === 'combined'
            ? `${task.name} ${failureRateLabel(task.kind)}`
            : task.name,
          type: 'line' as const,
          yAxisIndex: mode === 'combined'
            ? 1
            : 0,
          showSymbol: points.length === 1,
          symbolSize: 4,
          step: 'end' as const,
          lineStyle: {
            width: 1.4,
            type: 'dashed' as const,
            color: taskColor(task.id),
            opacity: 0.8,
          },
          areaStyle: {
            color: taskColor(task.id),
            opacity: 0.05,
          },
          itemStyle: {
            color: taskColor(task.id),
          },
          connectNulls: false,
          data: points,
          markLine: showThresholds.value && index === 0
            ? {
                silent: true,
                symbol: ['none', 'none'] as [string, string],
                label: { show: true, position: 'insideEndTop' as const, fontSize: 10 },
                lineStyle: { width: 1, type: 'dotted' as const, opacity: 0.65 },
                data: [
                  {
                    name: `${chartFailureLabel.value}警告`,
                    yAxis: chartThresholds.value.failureWarning,
                    lineStyle: { color: chartThresholds.value.warningColor },
                  },
                  {
                    name: `${chartFailureLabel.value}严重`,
                    yAxis: chartThresholds.value.failureCritical,
                    lineStyle: { color: chartThresholds.value.criticalColor },
                  },
                ],
              }
            : undefined,
        }
      })
    : []
  const latencyAxis = {
    type: 'value' as const,
    name: `${chartLatencyLabel.value} (ms)`,
    min: 0,
    nameTextStyle: { color: secondary },
    axisLabel: { fontSize: 11, color: secondary },
    axisLine: { show: false },
    axisTick: { show: false },
    splitLine: {
      lineStyle: { color: grid, type: 'dashed' as const },
    },
  }
  const failureAxis = {
    type: 'value' as const,
    name: `${chartFailureLabel.value} (%)`,
    min: 0,
    max: 100,
    position: mode === 'combined'
      ? 'right' as const
      : 'left' as const,
    nameTextStyle: { color: secondary },
    axisLabel: { fontSize: 11, color: secondary, formatter: '{value}%' },
    axisLine: { show: false },
    axisTick: { show: false },
    splitLine: {
      show: mode === 'failure',
      lineStyle: { color: grid, type: 'dashed' as const },
    },
  }
  return {
    darkMode: app.isDark,
    animation: false,
    color: selectedTasks.value.map(task => taskColor(task.id)),
    aria: { enabled: true },
    textStyle: { color: foreground },
    tooltip: {
      trigger: 'axis',
      renderMode: 'html',
      confine: true,
      backgroundColor: app.isDark ? 'rgba(40,40,40,0.95)' : 'rgba(255,255,255,0.8)',
      borderColor: 'transparent',
      borderWidth: 0,
      borderRadius: 6,
      textStyle: { color: foreground, fontSize: 12, lineHeight: 20 },
      axisPointer: {
        type: 'cross',
        crossStyle: { color: secondary },
        lineStyle: { color: border, width: 1, type: 'dashed' },
      },
      formatter: chartTooltip,
    },
    legend: {
      type: 'scroll',
      selectedMode: false,
      bottom: showZoom.value ? 24 : 0,
      itemWidth: 12,
      itemHeight: 8,
      itemGap: 14,
      icon: 'roundRect',
      textStyle: { fontSize: 10, color: secondary },
    },
    dataZoom: showZoom.value
      ? [
          { type: 'inside', start: 0, end: 100 },
          {
            type: 'slider',
            height: 14,
            bottom: 0,
            borderColor: 'transparent',
            fillerColor: app.isDark ? 'rgba(52,211,153,.16)' : 'rgba(5,150,105,.12)',
            backgroundColor: grid,
            showDetail: false,
          },
        ]
      : [
          { type: 'inside', start: 0, end: 100 },
        ],
    grid: {
      left: 56,
      right: mode === 'combined'
        ? 66
        : 24,
      top: 30,
      bottom: showZoom.value ? 72 : 52,
    },
    xAxis: {
      type: 'time',
      min: data.value?.range_start_unix_ms,
      max: data.value?.range_end_unix_ms,
      axisLabel: { fontSize: 11, color: secondary, margin: 12 },
      axisLine: {
        show: true,
        lineStyle: { color: border, width: 1 },
      },
      axisTick: { show: false },
      splitLine: { show: false },
    },
    yAxis: mode === 'combined'
      ? [latencyAxis, failureAxis]
      : [
          mode === 'latency'
            ? latencyAxis
            : failureAxis,
        ],
    series: [
      ...latencySeries,
      ...failureSeries,
    ],
  }
}

const combinedOption = computed<EChartsOption>(() => chartOption(chartMetric.value))
const latencyOption = computed<EChartsOption>(() => chartOption('latency'))
const failureOption = computed<EChartsOption>(() => chartOption('failure'))

function download(filename: string, href: string): void {
  const link = document.createElement('a')
  link.href = href
  link.download = filename
  document.body.append(link)
  link.click()
  link.remove()
}

const exportFilename = (extension: string) => `senlun-ping-${props.uuid.replace(filenameUnsafeChars, '_')}-${hours.value}h.${extension}`

function exportCsv(): void {
  if (!data.value || !selectedTasks.value.length)
    return
  const selected = selectedTasks.value
  const headers = [
    '时间',
    ...selected.flatMap(task => [`${task.name} 延迟(ms)`, `${task.name} ${failureRateLabel(task.kind)}(%)`]),
  ]
  const series = selected.map(task => ({
    latency: new Map(taskChartData(task.id)),
    failure: new Map(taskFailureData(task.id)),
  }))
  const times = [
    ...new Set(data.value.chart.filter(point => selectedTaskIds.value.includes(point.task_id)).map(point => point.received_at_unix_ms)),
  ].sort((a, b) => a - b)
  const escapeCsv = (value: string | number | null | undefined) => {
    const text = value == null
      ? ''
      : String(value)
    // Spreadsheet applications can execute formula-like fields, including task names in headers.
    const safe = spreadsheetFormulaPrefix.test(text)
      ? `'${text}`
      : text
    return `"${safe.replaceAll('"', '""')}"`
  }
  const rows = times.map(time => [
    new Date(time).toISOString(),
    ...series.flatMap(task => [task.latency.get(time) ?? '', task.failure.get(time) ?? '']),
  ])
  const csv = `\uFEFF${[headers, ...rows].map(row => row.map(escapeCsv).join(',')).join('\r\n')}`
  const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }))
  download(exportFilename('csv'), url)
  URL.revokeObjectURL(url)
  exportError.value = ''
}

function exportPng(): void {
  const target = chartLayout.value === 'combined'
    ? combinedChartRef.value
    : showDelay.value ? latencyChartRef.value : failureChartRef.value
  const chart = target as { getDataURL?: (options: { type: 'png', pixelRatio: number, backgroundColor: string }) => string } | null
  const url = chart?.getDataURL?.({
    type: 'png',
    pixelRatio: 2,
    backgroundColor: app.isDark ? '#111827' : '#ffffff',
  })
  if (!url) {
    exportError.value = '图表尚未准备好，请稍后再试。'
    return
  }
  download(exportFilename('png'), url)
  exportError.value = ''
}
</script>

<template>
  <div class="flex min-w-0 flex-col gap-4">
    <div class="flex min-w-0 flex-wrap items-center justify-between gap-2">
      <Tabs v-if="selectedHours === undefined" class="min-w-0 max-w-full flex-1" :model-value="String(hours)" @update:model-value="hours = Number($event)">
        <div class="min-w-0 overflow-x-auto">
          <TabsList :class="pickSurfaceClass('w-max h-8 bg-background/60 rounded-md', 'w-max h-8 bg-background/50 backdrop-blur-xl rounded-md')" aria-label="探测历史时间范围">
            <TabsTrigger
              v-for="range in availableRanges" :key="range" :value="String(range)"
              class="h-6.5 shrink-0 rounded-sm border-none text-xs shadow-none data-[state=active]:text-emerald-700 dark:data-[state=active]:text-emerald-400"
            >
              {{ rangeLabel(range) }}
            </TabsTrigger>
          </TabsList>
        </div>
      </Tabs>
      <div class="flex shrink-0 items-center gap-1">
        <Button variant="ghost" size="xs" class="h-8 rounded-md text-xs" :class="autoRefresh && 'text-emerald-700 dark:text-emerald-400'" @click="toggleAutoRefresh">
          <Icon :icon="autoRefresh ? 'lucide:pause' : 'lucide:play'" :width="13" :height="13" aria-hidden="true" />
          {{ autoRefresh ? '暂停刷新' : '自动刷新' }}
        </Button>
        <Button variant="ghost" size="icon-xs" class="size-8 rounded-md" aria-label="立即刷新" title="立即刷新" :disabled="loading" @click="reload">
          <Icon icon="lucide:refresh-cw" :class="loading && 'animate-spin'" :width="14" :height="14" aria-hidden="true" />
        </Button>
      </div>
    </div>
    <p v-if="loading" role="status" class="sr-only">
      正在读取探测记录…
    </p>
    <Spinner :show="loading" :aria-busy="loading" class="min-h-32" :description="loading && !data ? '正在读取探测记录…' : undefined">
      <div v-if="error" role="alert" class="flex flex-col items-center gap-3 py-8 text-center text-sm text-destructive">
        <span>{{ error }}</span>
        <Button variant="outline" size="sm" @click="reload">重新尝试</Button>
      </div>
      <Empty v-else-if="!loading && (!data || !tasks.length)" description="暂无延迟数据" />
      <template v-else-if="data && tasks.length">
        <div class="grid min-w-0 items-start gap-4 lg:grid-cols-[minmax(0,1fr)_23rem]">
          <section aria-label="线路趋势" class="min-w-0 rounded-xl border border-border/60" :class="pickSurfaceClass('bg-background/70', 'bg-background/50 backdrop-blur-xl')">
            <div class="flex min-w-0 flex-wrap items-center gap-3 px-4 pt-4 pb-2">
              <div class="min-w-0 flex-1">
                <h3 class="text-sm font-semibold">
                  {{ tasks.length }} 条线路 {{ chartMetric === 'failure' ? chartFailureLabel : chartLatencyLabel }} 趋势
                </h3>
                <p class="mt-1 text-[11px] text-muted-foreground">悬停对比同一时刻的线路数据</p>
              </div>
              <div role="group" aria-label="趋势指标" class="flex items-center rounded-lg bg-muted/70 p-1">
                <Button variant="ghost" size="xs" class="h-7 rounded-md px-3 text-xs" :class="chartMetric === 'latency' && 'bg-emerald-600 text-white hover:bg-emerald-600 hover:text-white'" :aria-pressed="chartMetric === 'latency'" @click="chartMetric = 'latency'">
                  {{ chartLatencyLabel }}
                </Button>
                <Button variant="ghost" size="xs" class="h-7 rounded-md px-3 text-xs" :class="chartMetric === 'failure' && 'bg-emerald-600 text-white hover:bg-emerald-600 hover:text-white'" :aria-pressed="chartMetric === 'failure'" @click="chartMetric = 'failure'">
                  {{ failureRateLabelForKinds(tasks.map(task => task.kind), true) }}
                </Button>
              </div>
              <SelectControl v-model="lineSelection" aria-label="显示线路" :options="lineOptions" class="w-32 text-xs" />
            </div>
            <template v-if="data.chart.length && selectedTasks.length">
              <div v-if="chartLayout === 'combined'" class="h-88 min-w-0 p-3 md:h-104">
                <VChart :key="`${app.resolvedThemeMode}:${chartMetric}`" ref="combinedChartRef" :option="combinedOption" autoresize style="height: 100%; width: 100%" :aria-label="mainChartLabel" />
              </div>
              <template v-else>
                <div v-if="showDelay" class="h-72 min-w-0 p-3 md:h-80">
                  <VChart :key="app.resolvedThemeMode" ref="latencyChartRef" :option="latencyOption" autoresize style="height: 100%; width: 100%" aria-label="独立延迟曲线，纵轴为毫秒，长时间缺报为断点" />
                </div>
                <div v-if="showFailures" class="h-72 min-w-0 p-3 md:h-80">
                  <VChart :key="app.resolvedThemeMode" ref="failureChartRef" :option="failureOption" autoresize style="height: 100%; width: 100%" :aria-label="`独立${chartFailureLabel}曲线，纵轴为百分比，长时间缺报为断点`" />
                </div>
              </template>
            </template>
            <div v-else class="flex h-88 items-center justify-center px-6 text-center text-sm text-muted-foreground md:h-104">
              {{ !selectedTasks.length ? '请选择要显示的线路。' : '此时间范围暂无上报样本。' }}
            </div>
            <details class="border-t border-border/50 px-4 py-3">
              <summary class="cursor-pointer text-xs font-medium text-muted-foreground">图表选项与导出</summary>
              <div class="mt-3 flex flex-wrap items-center gap-2">
                <Button variant="outline" size="xs" :aria-pressed="showDelay" @click="chartMetric = showDelay ? 'failure' : 'combined'">延迟</Button>
                <Button variant="outline" size="xs" :aria-pressed="showFailures" @click="chartMetric = showFailures ? 'latency' : 'combined'">{{ chartFailureLabel }}</Button>
                <Button variant="outline" size="xs" :aria-pressed="cutPeak" @click="cutPeak = !cutPeak">平滑峰值</Button>
                <DataTooltip content="使用 EWMA 算法平滑曲线并过滤突变值；汇总仍按原始样本计算" placement="top" content-class="max-w-56 text-[11px] leading-relaxed">
                  <button type="button" class="flex size-8 items-center justify-center rounded-md text-muted-foreground focus-visible:outline-2 focus-visible:outline-emerald-600" aria-label="平滑峰值说明">
                    <Icon icon="lucide:info" :width="14" :height="14" aria-hidden="true" />
                  </button>
                </DataTooltip>
                <Button variant="outline" size="xs" :aria-pressed="showThresholds" @click="showThresholds = !showThresholds">阈值线</Button>
                <Button variant="outline" size="xs" :aria-pressed="showPeaks" @click="showPeaks = !showPeaks">峰值标记</Button>
                <div role="group" aria-label="图表布局" class="flex items-center gap-1 rounded-md bg-muted/60 p-1">
                  <Button variant="ghost" size="xs" :class="chartLayout === 'combined' && 'bg-background text-emerald-700 dark:text-emerald-400'" :aria-pressed="chartLayout === 'combined'" @click="chartLayout = 'combined'">合图</Button>
                  <Button variant="ghost" size="xs" :class="chartLayout === 'split' && 'bg-background text-emerald-700 dark:text-emerald-400'" :aria-pressed="chartLayout === 'split'" @click="chartLayout = 'split'">分图</Button>
                </div>
                <Button variant="outline" size="xs" :disabled="!data.chart.length || !selectedTasks.length" @click="exportPng">
                  <Icon icon="lucide:image-down" :width="13" :height="13" aria-hidden="true" />PNG
                </Button>
                <Button variant="outline" size="xs" :disabled="!data.chart.length || !selectedTasks.length" @click="exportCsv">
                  <Icon icon="lucide:file-down" :width="13" :height="13" aria-hidden="true" />CSV
                </Button>
              </div>
              <p v-if="data.history_resolution_ms" class="mt-3 text-xs text-muted-foreground">较早历史按分钟汇总，峰值、失败次数和延迟分位数保留。</p>
            </details>
            <p v-if="exportError" role="alert" class="px-4 pb-3 text-sm text-destructive">{{ exportError }}</p>
          </section>
          <section aria-label="延迟数据分析" class="min-w-0 rounded-xl border border-border/60" :class="pickSurfaceClass('bg-background/70', 'bg-background/50 backdrop-blur-xl')">
            <div class="flex flex-wrap items-center justify-between gap-2 px-4 pt-4 pb-3">
              <h3 class="text-sm font-semibold">线路检查器</h3>
              <div class="flex items-center gap-1">
                <Button variant="ghost" size="xs" class="h-7 px-2 text-[11px]" @click="selectedTaskIds = tasks.map(task => task.id)">全选</Button>
                <Button variant="ghost" size="xs" class="h-7 px-2 text-[11px]" @click="selectedTaskIds = []">全不选</Button>
              </div>
            </div>
            <div class="px-1 pb-1">
              <div
                v-for="task in tasks" :key="task.id" role="group" :aria-label="`线路 ${task.name}`"
                class="grid min-w-0 grid-cols-[minmax(0,1fr)_4.5rem_3rem_2.75rem] items-center sm:grid-cols-[minmax(0,1fr)_4.5rem_3rem_3rem_2.75rem] gap-1 rounded-lg border-t border-border/40 px-3 py-2 transition-colors"
                :class="[!selectedTaskIds.includes(task.id) && 'opacity-40', analysisTaskId === task.id && 'bg-emerald-500/5']"
              >
                <button type="button" class="min-w-0 cursor-pointer rounded-md py-1 text-left focus-visible:outline-2 focus-visible:outline-emerald-600" :aria-label="`分析 ${task.name}`" :aria-pressed="analysisTaskId === task.id" @click="analysisTaskId = task.id">
                  <span class="flex min-w-0 items-center gap-1.5">
                    <span class="size-1.5 shrink-0 rounded-full" :style="{ backgroundColor: taskColor(task.id) }" aria-hidden="true" />
                    <span class="min-w-0 text-xs font-semibold break-words sm:truncate" :title="task.name">{{ task.name.replace(/ipv6/gi, 'V6') }}</span>
                  </span>
                  <span class="mt-1 inline-flex rounded px-1.5 py-0.5 text-[10px] font-medium" :class="lineStatus(task.id).className" :title="`按当前时间范围的平均延迟和${failureRateLabel(task.kind)}判断`">{{ lineStatus(task.id).label }}</span>
                </button>
                <div class="min-w-0">
                  <span class="block text-[10px] text-muted-foreground">平均 {{ chartLatencyLabel }}</span>
                  <span class="block whitespace-nowrap text-[11px] font-semibold tabular-nums">{{ cardLatency(summaryByTask.get(task.id)?.avg_latency_ms) }}</span>
                </div>
                <div class="min-w-0">
                  <span class="block truncate text-[10px] text-muted-foreground">{{ failureRateLabel(task.kind, true) }}</span>
                  <span class="block whitespace-nowrap text-[11px] font-semibold tabular-nums">{{ summaryByTask.get(task.id)?.samples ? `${summaryByTask.get(task.id)!.loss_percent.toFixed(2)}%` : '--' }}</span>
                </div>
                <svg viewBox="0 0 64 28" class="hidden h-7 w-12 sm:block" :style="{ color: taskColor(task.id) }" aria-hidden="true" focusable="false">
                  <path :d="sparklines.get(task.id)?.area" fill="currentColor" fill-opacity="0.08" />
                  <path :d="sparklines.get(task.id)?.line" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
                </svg>
                <button type="button" class="flex size-11 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-emerald-600" :aria-label="`${selectedTaskIds.includes(task.id) ? '隐藏' : '显示'} ${task.name} 曲线`" :aria-pressed="selectedTaskIds.includes(task.id)" @click="toggleTask(task.id)">
                  <Icon :icon="selectedTaskIds.includes(task.id) ? 'lucide:eye' : 'lucide:eye-off'" :width="15" :height="15" aria-hidden="true" />
                </button>
              </div>
            </div>
            <details v-if="analysisTask" class="border-t border-border/50 px-4 py-3">
              <summary class="cursor-pointer text-xs font-medium text-muted-foreground">详细统计</summary>
              <div class="mt-3 flex flex-col gap-4">
                <label class="flex items-center justify-between gap-2 text-xs text-muted-foreground">
                  分析任务
                  <SelectControl v-model="analysisTaskId" aria-label="分析任务" :options="tasks.map(task => ({ value: task.id, label: task.name }))" class="max-w-44" />
                </label>
                <div class="flex min-w-0 items-center justify-between gap-2">
                  <h4 class="truncate text-sm font-semibold" :title="analysisTask.name">{{ analysisTask.name }}</h4>
                  <Button variant="ghost" size="xs" class="shrink-0" @click="selectedTaskIds = [analysisTask.id]">仅看此任务</Button>
                </div>
                <dl class="grid grid-cols-2 gap-x-3 gap-y-4 sm:grid-cols-4 lg:grid-cols-2">
                  <div v-for="metric in [
                    { label: '最小延迟', value: analysisLatency(analysisSummary?.min_latency_ms) },
                    { label: '平均延迟', value: analysisLatency(analysisSummary?.avg_latency_ms) },
                    { label: '最大延迟', value: analysisLatency(analysisSummary?.max_latency_ms) },
                    { label: '最新延迟', value: analysisLatency(analysisSummary?.latest_latency_ms) },
                    { label: 'P50', value: analysisLatency(analysisSummary?.p50_latency_ms) },
                    { label: 'P95', value: analysisLatency(analysisSummary?.p95_latency_ms) },
                    { label: 'P99', value: analysisLatency(analysisSummary?.p99_latency_ms) },
                    { label: failureRateLabel(analysisTask.kind), value: analysisSummary?.samples ? `${analysisSummary.loss_percent.toFixed(2)}%` : '无样本' },
                    { label: '波动率', value: analysisSummary?.p99_p50_ratio == null ? '—' : analysisSummary.p99_p50_ratio.toFixed(2) },
                    { label: '样本数', value: String(analysisSummary?.samples ?? 0) },
                    { label: '采集间隔', value: `${analysisTask.interval_seconds}s` },
                    { label: '协议', value: analysisTask.kind.toUpperCase() },
                  ]" :key="metric.label">
                    <dt class="text-[11px] text-muted-foreground">{{ metric.label }}</dt>
                    <dd class="mt-0.5 text-sm font-semibold tabular-nums">{{ metric.value }}</dd>
                  </div>
                </dl>
              </div>
            </details>
          </section>
        </div>
        <details v-if="recent.length" class="relative rounded-md border p-3 bg-background/50">
          <summary class="cursor-pointer text-sm font-medium">
            查看最近 {{ recent.length }} 条记录
          </summary>
          <div class="mt-3 max-h-96 overflow-auto">
            <table class="w-full text-left text-sm">
              <caption class="sr-only">
                探测结果明细（按服务器接收时间倒序）
              </caption><thead>
                <tr>
                  <th class="p-2">
                    接收时间
                  </th><th class="p-2">
                    任务
                  </th><th class="p-2">
                    结果
                  </th>
                </tr>
              </thead><tbody>
                <tr v-for="(record, index) in recent" :key="index" class="border-t">
                  <td class="whitespace-nowrap p-2">
                    {{ date(record.received_at_unix_ms) }}
                  </td><td class="p-2">
                    {{ names.get(record.task_id) ?? record.task_id }}
                  </td><td class="p-2" :class="record.success ? '' : 'text-destructive'">
                    <template v-if="record.aggregated">
                      <span class="block">分钟汇总 · {{ record.samples }} 次，失败 {{ record.failed_samples }} 次</span>
                      <span class="text-xs text-muted-foreground">平均延迟 {{ latency(record.latency_ms) }}</span>
                    </template>
                    <template v-else>
                      {{ record.success ? latency(record.latency_ms) : `失败：${record.error || '无响应'}` }}
                    </template>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </details>
      </template>
    </Spinner>
  </div>
</template>
