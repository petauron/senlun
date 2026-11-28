import type { AlertRule, ManagedNode, NotificationChannel, ProbeTask } from './admin'

export interface AssignableNodeOption {
  id: string
  label: string
  revoked: boolean
  missing: boolean
}

export interface SelectableChannelOption {
  id: string
  label: string
  missing: boolean
}

export interface NodeTargetScopeSummary {
  mode: 'all' | 'scoped'
  label: string
  totalCount: number
  activeCount: number
  revokedCount: number
  missingCount: number
  warningLabel: string
}

export interface AlertChannelSummary {
  mode: 'in-app-only' | 'channels'
  label: string
  totalCount: number
  enabledCount: number
  disabledCount: number
  missingCount: number
  warningLabel: string
}

export interface ScopedNodeValidationInput {
  guardGlobalScope: boolean
  selectedIds: readonly string[]
  enabled: boolean
  confirmGlobalScope: boolean
  nodes: readonly ManagedNode[]
}

const MAX_PUBLIC_REMARK_BYTES = 2048

export function buildAssignableNodes(
  nodes: readonly ManagedNode[],
  selectedIds: readonly string[],
  persistedIds: readonly string[] = [],
): AssignableNodeOption[] {
  const byId = new Map(nodes.map(node => [node.id, node]))
  const options: AssignableNodeOption[] = []
  const seen = new Set<string>()

  for (const node of nodes) {
    if (node.disabled)
      continue
    seen.add(node.id)
    options.push({
      id: node.id,
      label: node.name,
      revoked: false,
      missing: false,
    })
  }

  const candidateIds = [
    ...selectedIds,
    ...persistedIds,
  ]
  for (const id of candidateIds) {
    if (seen.has(id))
      continue
    seen.add(id)
    const node = byId.get(id)
    if (node) {
      options.push({
        id: node.id,
        label: `${node.name}（凭据已撤销）`,
        revoked: true,
        missing: false,
      })
    }
    else {
      options.push({
        id,
        label: `${id}（节点已不存在）`,
        revoked: false,
        missing: true,
      })
    }
  }

  return options
}

export function buildSelectableChannels(
  channels: readonly NotificationChannel[],
  selectedIds: readonly string[],
): SelectableChannelOption[] {
  const known = new Set<string>()
  const options: SelectableChannelOption[] = []

  for (const channel of channels) {
    known.add(channel.id)
    options.push({
      id: channel.id,
      label: channel.enabled
        ? channel.name
        : `${channel.name}（已停用）`,
      missing: false,
    })
  }

  const seenMissing = new Set<string>()
  for (const id of selectedIds) {
    if (known.has(id) || seenMissing.has(id))
      continue
    seenMissing.add(id)
    options.push({
      id,
      label: `${id}（渠道已失效）`,
      missing: true,
    })
  }

  return options
}

export function pruneMissingChannels(
  selectedIds: readonly string[],
  channels: readonly NotificationChannel[],
): string[] {
  const known = new Set(channels.map(channel => channel.id))
  return selectedIds.filter(id => known.has(id))
}

export function hasUnavailableNodes(
  selectedIds: readonly string[],
  nodes: readonly ManagedNode[],
): boolean {
  const active = new Set(nodes.filter(node => !node.disabled).map(node => node.id))
  return selectedIds.some(id => !active.has(id))
}

/**
 * Require explicit opt-in before enabling an existing item with an empty node
 * list when it previously targeted explicit nodes or was disabled with no
 * targets (including items disabled after removing or deleting their last node).
 * New items and already-enabled global items keep the existing editing flow.
 */
export function shouldGuardGlobalScope(item: { id: string, enabled: boolean, node_ids: readonly string[] }): boolean {
  return Boolean(item.id) && (item.node_ids.length > 0 || !item.enabled)
}

export function requiresGlobalScopeConfirmation(
  guardGlobalScope: boolean,
  selectedIds: readonly string[],
  enabled: boolean,
): boolean {
  return guardGlobalScope && selectedIds.length === 0 && enabled
}

export function validateScopedNodeSelection(input: ScopedNodeValidationInput): string | null {
  if (hasUnavailableNodes(input.selectedIds, input.nodes))
    return '请先取消勾选已撤销或不存在的节点后再保存。'
  if (
    requiresGlobalScopeConfirmation(
      input.guardGlobalScope,
      input.selectedIds,
      input.enabled,
    )
    && !input.confirmGlobalScope
  ) {
    return '为避免静默扩大到所有活动节点，请重新选择活动节点、保持停用，或勾选确认改为应用于所有活动节点。'
  }
  return null
}

export function validateAlertDraftChannels(
  selectedChannelIds: readonly string[],
  channels: readonly NotificationChannel[],
): string | null {
  const known = new Set(channels.map(channel => channel.id))
  if (selectedChannelIds.some(id => !known.has(id)))
    return '草稿包含已失效的通知渠道，请先取消勾选或移除失效渠道后再保存。'
  return null
}

export function normalizePublicRemark(value: string): string {
  return value.replaceAll('\r\n', '\n')
}

function hasDisallowedControlChar(value: string): boolean {
  for (const ch of value) {
    const code = ch.codePointAt(0) ?? 0
    if ((code <= 0x1F && code !== 0x0A) || (code >= 0x7F && code <= 0x9F))
      return true
  }
  return false
}

export function validatePublicRemark(value: string): string | null {
  const normalized = normalizePublicRemark(value)
  if (hasDisallowedControlChar(normalized))
    return '公开备注仅支持普通文本和换行，不能包含制表符或其他控制字符。'
  if (new TextEncoder().encode(normalized).length > MAX_PUBLIC_REMARK_BYTES)
    return '公开备注长度不能超过 2048 字节。'
  return null
}

function sameNumericField(a: number | string | undefined, b: number | string | undefined): boolean {
  const sa = String(a ?? '').trim()
  const sb = String(b ?? '').trim()
  if (sa === sb)
    return true
  if (!sa || !sb)
    return false
  const na = Number(sa)
  const nb = Number(sb)
  return Number.isFinite(na) && Number.isFinite(nb) && na === nb
}

function sameIdSet(a: readonly string[], b: readonly string[]): boolean {
  const setA = new Set(a)
  const setB = new Set(b)
  if (setA.size !== setB.size)
    return false
  for (const id of setA) {
    if (!setB.has(id))
      return false
  }
  return true
}

export function cloneProbeTask(task: ProbeTask): ProbeTask {
  return {
    ...task,
    node_ids: [...task.node_ids],
  }
}

export function isProbeDraftDirty(
  current: ProbeTask,
  baseline: ProbeTask,
  confirmGlobalScope = false,
): boolean {
  if (confirmGlobalScope)
    return true
  return !(
    current.id === baseline.id
    && current.name === baseline.name
    && sameNumericField(current.weight, baseline.weight)
    && current.kind === baseline.kind
    && current.target === baseline.target
    && sameNumericField(current.interval_seconds, baseline.interval_seconds)
    && sameNumericField(current.timeout_seconds, baseline.timeout_seconds)
    && Boolean(current.enabled) === Boolean(baseline.enabled)
    && sameIdSet(current.node_ids, baseline.node_ids)
  )
}

export function cloneNotificationChannel(channel: NotificationChannel): NotificationChannel {
  return { ...channel }
}

export function isChannelDraftDirty(
  current: NotificationChannel,
  baseline: NotificationChannel,
): boolean {
  return !(
    current.id === baseline.id
    && current.name === baseline.name
    && current.kind === baseline.kind
    && current.url === baseline.url
    && Boolean(current.enabled) === Boolean(baseline.enabled)
  )
}

export function cloneAlertRule(rule: AlertRule): AlertRule {
  return {
    ...rule,
    node_ids: [...rule.node_ids],
    channel_ids: [...rule.channel_ids],
  }
}

export function isAlertRuleDraftDirty(
  current: AlertRule,
  baseline: AlertRule,
  confirmGlobalScope = false,
): boolean {
  if (confirmGlobalScope)
    return true
  return !(
    current.id === baseline.id
    && current.name === baseline.name
    && current.metric === baseline.metric
    && sameNumericField(current.threshold, baseline.threshold)
    && sameNumericField(current.duration_seconds, baseline.duration_seconds)
    && sameNumericField(current.cooldown_seconds, baseline.cooldown_seconds)
    && Boolean(current.enabled) === Boolean(baseline.enabled)
    && sameIdSet(current.node_ids, baseline.node_ids)
    && sameIdSet(current.channel_ids, baseline.channel_ids)
  )
}

export function summarizeNodeTargetScope(
  nodeIds: readonly string[],
  nodes: readonly ManagedNode[],
): NodeTargetScopeSummary {
  const uniqueIds = Array.from(new Set(nodeIds))
  if (uniqueIds.length === 0) {
    return {
      mode: 'all',
      label: '全部活动节点',
      totalCount: 0,
      activeCount: 0,
      revokedCount: 0,
      missingCount: 0,
      warningLabel: '',
    }
  }
  const byId = new Map(nodes.map(node => [node.id, node]))
  let activeCount = 0
  let revokedCount = 0
  let missingCount = 0
  for (const id of uniqueIds) {
    const node = byId.get(id)
    if (!node)
      missingCount++
    else if (node.disabled)
      revokedCount++
    else
      activeCount++
  }
  const warningParts: string[] = []
  if (revokedCount > 0)
    warningParts.push(`${revokedCount} 个已撤销`)
  if (missingCount > 0)
    warningParts.push(`${missingCount} 个已不存在`)
  return {
    mode: 'scoped',
    label: `指定 ${uniqueIds.length} 个节点`,
    totalCount: uniqueIds.length,
    activeCount,
    revokedCount,
    missingCount,
    warningLabel: warningParts.length > 0
      ? `含 ${warningParts.join('、')}节点`
      : '',
  }
}

export function summarizeAlertChannels(
  channelIds: readonly string[],
  channels: readonly NotificationChannel[],
): AlertChannelSummary {
  const uniqueIds = Array.from(new Set(channelIds))
  if (uniqueIds.length === 0) {
    return {
      mode: 'in-app-only',
      label: '仅站内记录',
      totalCount: 0,
      enabledCount: 0,
      disabledCount: 0,
      missingCount: 0,
      warningLabel: '',
    }
  }
  const byId = new Map(channels.map(channel => [channel.id, channel]))
  let enabledCount = 0
  let disabledCount = 0
  let missingCount = 0
  const knownNames: string[] = []
  for (const id of uniqueIds) {
    const channel = byId.get(id)
    if (!channel) {
      missingCount++
    }
    else {
      knownNames.push(channel.name)
      if (channel.enabled)
        enabledCount++
      else
        disabledCount++
    }
  }
  let label = `通知渠道：${uniqueIds.length} 个（均已失效）`
  if (knownNames.length === 1 && uniqueIds.length === 1)
    label = `通知渠道：${knownNames[0]}`
  else if (knownNames.length > 0)
    label = `通知渠道：${knownNames[0]} 等 ${uniqueIds.length} 个`

  const warningParts: string[] = []
  if (missingCount > 0)
    warningParts.push(`${missingCount} 个已失效`)
  if (disabledCount > 0)
    warningParts.push(`${disabledCount} 个已停用`)
  return {
    mode: 'channels',
    label,
    totalCount: uniqueIds.length,
    enabledCount,
    disabledCount,
    missingCount,
    warningLabel: warningParts.length > 0
      ? `含 ${warningParts.join('、')}渠道`
      : '',
  }
}

export function scrollAndFocusHeading(el: HTMLElement | null): void {
  if (!el)
    return
  const reducedMotion = typeof window !== 'undefined'
    && typeof window.matchMedia === 'function'
    && window.matchMedia('(prefers-reduced-motion: reduce)').matches
  if (typeof el.scrollIntoView === 'function') {
    el.scrollIntoView({
      behavior: reducedMotion
        ? 'auto'
        : 'smooth',
      block: 'start',
    })
  }
  el.focus({ preventScroll: true })
}
