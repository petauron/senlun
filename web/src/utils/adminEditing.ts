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

export type NodeScopeMode = 'scoped' | 'all'

export interface ScopedNodeValidationInput {
  guardGlobalScope: boolean
  scopeMode: NodeScopeMode
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

export function initialNodeScopeMode(item: { id: string, node_ids: readonly string[] }): NodeScopeMode {
  return item.id && item.node_ids.length === 0
    ? 'all'
    : 'scoped'
}

export function effectiveScopedNodeIds(
  scopeMode: NodeScopeMode,
  selectedIds: readonly string[],
): string[] {
  return scopeMode === 'all'
    ? []
    : [...selectedIds]
}

export function pruneUnavailableNodes(
  selectedIds: readonly string[],
  nodes: readonly ManagedNode[],
): string[] {
  const active = new Set(nodes.filter(node => !node.disabled).map(node => node.id))
  return selectedIds.filter(id => active.has(id))
}

export function filterAssignableActiveNodes(
  options: readonly AssignableNodeOption[],
  nodes: readonly ManagedNode[],
  query: string,
  selectedIds: readonly string[],
  selectedOnly: boolean,
): AssignableNodeOption[] {
  const byId = new Map(nodes.map(node => [node.id, node]))
  const selected = new Set(selectedIds)
  const normalizedQuery = query.trim().toLowerCase()
  return options.filter((option) => {
    if (option.revoked || option.missing)
      return false
    if (selectedOnly)
      return selected.has(option.id)
    if (!normalizedQuery)
      return true
    const node = byId.get(option.id)
    return (
      option.label.toLowerCase().includes(normalizedQuery)
      || option.id.toLowerCase().includes(normalizedQuery)
      || (node?.name.toLowerCase().includes(normalizedQuery) ?? false)
      || (node?.region.toLowerCase().includes(normalizedQuery) ?? false)
      || (node?.group.toLowerCase().includes(normalizedQuery) ?? false)
    )
  })
}

export function applyFilteredNodeSelection(
  selectedIds: readonly string[],
  visibleActiveIds: readonly string[],
  action: 'select' | 'deselect',
): string[] {
  if (action === 'select') {
    const next = [...selectedIds]
    const seen = new Set(selectedIds)
    for (const id of visibleActiveIds) {
      if (!seen.has(id)) {
        seen.add(id)
        next.push(id)
      }
    }
    return next
  }
  const remove = new Set(visibleActiveIds)
  return selectedIds.filter(id => !remove.has(id))
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
  scopeMode: NodeScopeMode,
  enabled: boolean,
): boolean {
  return guardGlobalScope && scopeMode === 'all' && enabled
}

export function validateScopedNodeSelection(input: ScopedNodeValidationInput): string | null {
  if (input.scopeMode === 'scoped') {
    if (hasUnavailableNodes(input.selectedIds, input.nodes))
      return '请先取消勾选或移除已撤销或不存在的节点后再保存。'
    if (input.selectedIds.length === 0)
      return '当前为“指定节点”模式，请至少选择 1 台活动节点；如需应用于所有活动节点，请主动切换至“所有活动节点（含之后注册）”。'
    return null
  }
  if (
    requiresGlobalScopeConfirmation(
      input.guardGlobalScope,
      input.scopeMode,
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
  currentScopeMode: NodeScopeMode = initialNodeScopeMode(current),
  baselineScopeMode: NodeScopeMode = initialNodeScopeMode(baseline),
): boolean {
  if (confirmGlobalScope || currentScopeMode !== baselineScopeMode)
    return true
  const currentNodes = effectiveScopedNodeIds(currentScopeMode, current.node_ids)
  const baselineNodes = effectiveScopedNodeIds(baselineScopeMode, baseline.node_ids)
  return !(
    current.id === baseline.id
    && current.name === baseline.name
    && sameNumericField(current.weight, baseline.weight)
    && current.kind === baseline.kind
    && current.target === baseline.target
    && sameNumericField(current.interval_seconds, baseline.interval_seconds)
    && sameNumericField(current.timeout_seconds, baseline.timeout_seconds)
    && Boolean(current.enabled) === Boolean(baseline.enabled)
    && sameIdSet(currentNodes, baselineNodes)
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
  currentScopeMode: NodeScopeMode = initialNodeScopeMode(current),
  baselineScopeMode: NodeScopeMode = initialNodeScopeMode(baseline),
): boolean {
  if (confirmGlobalScope || currentScopeMode !== baselineScopeMode)
    return true
  const currentNodes = effectiveScopedNodeIds(currentScopeMode, current.node_ids)
  const baselineNodes = effectiveScopedNodeIds(baselineScopeMode, baseline.node_ids)
  return !(
    current.id === baseline.id
    && current.name === baseline.name
    && current.metric === baseline.metric
    && sameNumericField(current.threshold, baseline.threshold)
    && sameNumericField(current.duration_seconds, baseline.duration_seconds)
    && sameNumericField(current.cooldown_seconds, baseline.cooldown_seconds)
    && Boolean(current.enabled) === Boolean(baseline.enabled)
    && sameIdSet(currentNodes, baselineNodes)
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
  const warningParts: string[] = []
  if (missingCount > 0)
    warningParts.push(`${missingCount} 个已失效`)
  if (disabledCount > 0)
    warningParts.push(`${disabledCount} 个已停用`)
  const warningLabel = warningParts.length > 0
    ? `含 ${warningParts.join('、')}渠道`
    : ''

  // Zero enabled delivery channels means alerts only record in-app (matches backend notifications.rs).
  if (enabledCount === 0) {
    return {
      mode: 'in-app-only',
      label: '仅站内记录（无启用外发渠道）',
      totalCount: uniqueIds.length,
      enabledCount,
      disabledCount,
      missingCount,
      warningLabel,
    }
  }

  let label = `通知渠道：${knownNames[0]} 等 ${uniqueIds.length} 个`
  if (knownNames.length === 1 && uniqueIds.length === 1)
    label = `通知渠道：${knownNames[0]}`

  return {
    mode: 'channels',
    label,
    totalCount: uniqueIds.length,
    enabledCount,
    disabledCount,
    missingCount,
    warningLabel,
  }
}

export function createClientUuid(): string {
  const bytes = new Uint8Array(16)
  if (typeof globalThis.crypto?.getRandomValues === 'function') {
    globalThis.crypto.getRandomValues(bytes)
  }
  else {
    for (let index = 0; index < bytes.length; index++)
      bytes[index] = Math.floor(Math.random() * 256)
  }
  bytes[6] = (bytes[6]! & 0x0F) | 0x40
  bytes[8] = (bytes[8]! & 0x3F) | 0x80
  const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
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
