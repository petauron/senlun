import type { ManagedNode, NotificationChannel } from './admin'

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
