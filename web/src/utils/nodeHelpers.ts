import type { NodeData, TrafficLimitType } from '@/stores/nodes'
import type { NodeProbe, SelectedTraffic } from '@/utils/rpc'
import { formatDateTime } from '@/utils/helper'
import { formatPriceWithCycle, getDaysUntilExpired, getExpireStatus, getExpireTextClass } from '@/utils/tagHelper'

export interface PriceTagItem {
  text: string
  highlight?: boolean
  prefix?: string
  value?: string
  suffix?: string
}

export function hasRegion(region: string | null | undefined): boolean {
  return Boolean(region?.trim())
}

export function calculateTrafficUsed(upload: number, download: number, type: TrafficLimitType): number {
  switch (type) {
    case 'up': return upload
    case 'down': return download
    case 'min': return Math.min(upload, download)
    case 'max': return Math.max(upload, download)
    case 'sum':
    default: return upload + download
  }
}

export function getTrafficUsed(node: NodeData): number {
  const { net_total_up = 0, net_total_down = 0, traffic_limit_type } = node
  return calculateTrafficUsed(net_total_up, net_total_down, traffic_limit_type)
}

export function trafficSourceLabel(selection: SelectedTraffic | undefined): string {
  if (!selection)
    return '流量来源不可用'
  return selection.source === 'provider' ? '服务商流量' : '本地流量'
}

export function trafficSelectionMessage(selection: SelectedTraffic | undefined): string {
  if (!selection)
    return '用量不可判定'
  const labels: Record<SelectedTraffic['status'], string> = {
    ready: selection.source === 'provider' ? '周期未知，按服务商快照计费值' : '',
    unlimited: '本地未设额度',
    unconfigured: '未配置服务商来源，不回退本地',
    disabled: '服务商同步已停用，显示上次值',
    authentication_required: '认证需管理员处理，当前用量不可判定',
    sync_failed: '同步失败，显示上次成功值',
    no_data: selection.source === 'provider' ? '尚无服务商快照' : '尚无本周期本地采样',
    stale: '同步已过期，显示上次成功值',
    quota_missing: '服务商未提供可用额度',
  }
  return labels[selection.status]
}

export function trafficSelectionDetails(selection: SelectedTraffic | undefined): string {
  const details = [trafficSourceLabel(selection), trafficSelectionMessage(selection)].filter(Boolean)
  if (selection?.source === 'provider') {
    details.push(selection.last_success_unix_ms ? `上次成功：${new Date(selection.last_success_unix_ms).toLocaleString()}` : '尚无成功同步')
    if (selection.reset_at)
      details.push(`重置记录：${selection.reset_at}（完整周期未知）`)
  }
  return details.join(' · ')
}

export function getPriceTags(node: NodeData, lang: 'zh-CN' | 'en-US', view: 'card' | 'list' = 'card'): PriceTagItem[] {
  const tags: PriceTagItem[] = []
  const priceText = formatPriceWithCycle(node.price, node.billing_cycle, node.currency, lang)
  tags.push({ text: priceText })
  if (!node.expired_at || !Number.isFinite(Date.parse(node.expired_at)))
    return tags
  const days = getDaysUntilExpired(node.expired_at)
  const status = getExpireStatus(node.expired_at)
  if (view === 'list' && status !== 'long_term')
    tags.push({ text: `${days >= 0 ? '+' : ''}${days}${lang === 'zh-CN' ? '天' : ' days'}`, highlight: true })
  else if (status === 'expired')
    tags.push({ text: lang === 'zh-CN' ? '已过期' : 'Expired', highlight: true })
  else if (status === 'long_term')
    tags.push({ text: lang === 'zh-CN' ? '长期' : 'Long-term' })
  else if (lang === 'zh-CN')
    tags.push({ text: `余 ${days} 天`, prefix: '余 ', value: String(days), suffix: ' 天' })
  else
    tags.push({ text: `${days} days left`, value: String(days), suffix: ' days left' })
  return tags
}

export function getRemainingTimeTagClass(node: NodeData): string {
  if (!node.expired_at || !Number.isFinite(Date.parse(node.expired_at)))
    return ''
  return getExpireTextClass(node.expired_at)
}

export function formatOfflineTime(node: NodeData): string {
  return formatDateTime(node.time)
}

export function getMemPercentage(node: NodeData): number {
  return (node.ram ?? 0) / (node.mem_total || 1) * 100
}

export function getDiskPercentage(node: NodeData): number {
  return (node.disk ?? 0) / (node.disk_total || 1) * 100
}

const probeToneKeys = ['pingExcellentColor', 'pingGoodColor', 'pingModerateColor', 'pingWarningColor', 'pingCriticalColor'] as const
const probeToneDefaults = ['#5EEAA6', '#47B592', '#A3E635', '#FACC15', '#F43F5E'] as const
const hexColor = /^#[0-9a-f]{6}$/i

export function getProbeToneColor(metric: 'latency' | 'loss', value: number | null, settings?: Record<string, unknown> | null): string | undefined {
  if (value === null)
    return undefined
  const limits = metric === 'latency' ? [60, 100, 160, 200] : [1, 3, 6, 9]
  const index = limits.findIndex(limit => value <= limit)
  const tone = index < 0 ? 4 : index
  const configured = settings?.[probeToneKeys[tone]!]
  return typeof configured === 'string' && hexColor.test(configured) ? configured.toUpperCase() : probeToneDefaults[tone]!
}

const ipv6ProbeName = /\b(?:ipv6|v6)\b/i

export function selectHomeProbes(probes: NodeProbe[], nodeUuid: string, settings?: Record<string, unknown> | null, ipVersion: 'ipv4' | 'ipv6' = 'ipv4'): Array<NodeProbe | null> {
  const configuredCount = settings?.homePingRowCount
  const rowCount = typeof configuredCount === 'number' && Number.isFinite(configuredCount)
    ? Math.round(Math.min(3, Math.max(1, configuredCount)))
    : 3
  const configuredMap = settings?.homePingTasksByNode
  const taskIds = configuredMap && typeof configuredMap === 'object' && !Array.isArray(configuredMap) && Object.hasOwn(configuredMap, nodeUuid)
    ? (configuredMap as Record<string, unknown>)[nodeUuid]
    : undefined
  const selected = Array.isArray(taskIds)
    ? Array.from(new Set(taskIds.filter(id => typeof id === 'string' || (typeof id === 'number' && Number.isInteger(id) && id >= 0)).map(String)), id => probes.find(task => task.id === id) ?? null)
    : probes
  const matching = selected.filter(task => !task || ipv6ProbeName.test(task.name) === (ipVersion === 'ipv6'))
  return Array.from({ length: rowCount }, (_, index) => matching[index] ?? null)
}
