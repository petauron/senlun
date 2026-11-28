import type { ManagedNode } from './admin'
import type { NodeDangerActionKind } from './adminUncertain'

export type NodeListFilter = 'all' | 'hidden' | 'disabled' | 'provider'

export interface NodeListFilterOption {
  value: NodeListFilter
  label: string
}

export const NODE_LIST_FILTER_OPTIONS: NodeListFilterOption[] = [
  { value: 'all', label: '全部' },
  { value: 'hidden', label: '已隐藏' },
  { value: 'disabled', label: '凭据已撤销' },
  { value: 'provider', label: '服务商流量' },
]

export type NodeSelectionResolution
  = | { mode: 'empty' }
    | { mode: 'list' }
    | { mode: 'missing', requestedId: string }
    | { mode: 'selected', node: ManagedNode }

export interface NodeSelectionContext {
  nodes: ManagedNode[]
  queryNodeId: string | null | undefined
  pinnedNodeId?: string | null | undefined
  isDesktop: boolean
}

export interface NodeDangerActionMeta {
  action: NodeDangerActionKind
  label: string
  confirmLabel: string
  description: (nodeName: string) => string
}

export const NODE_DANGER_ACTION_ORDER: NodeDangerActionKind[] = [
  'rotate',
  'revoke',
  'reset-traffic',
  'delete',
]

export const NODE_DANGER_ACTIONS: Record<NodeDangerActionKind, NodeDangerActionMeta> = {
  'rotate': {
    action: 'rotate',
    label: '轮换 Agent 凭据',
    confirmLabel: '确认轮换凭据',
    description: nodeName => `节点「${nodeName}」的旧 Agent 凭据会立即失效。请保存新凭据并更新对应 Agent，否则节点将停止上报。`,
  },
  'revoke': {
    action: 'revoke',
    label: '撤销 Agent 凭据',
    confirmLabel: '确认撤销凭据',
    description: nodeName => `节点「${nodeName}」的对应 Agent 将立即无法继续上报；已撤销的旧节点与历史记录会保留，不会因重新注册恢复。如需重新接入，请生成一次性注册令牌注册为独立新节点。`,
  },
  'reset-traffic': {
    action: 'reset-traffic',
    label: '重置本周期流量',
    confirmLabel: '确认重置流量',
    description: nodeName => `节点「${nodeName}」的本周期本地流量累计将归零，不会重置主机的网络接口计数，也不会修改服务商外部流量。`,
  },
  'delete': {
    action: 'delete',
    label: '删除节点',
    confirmLabel: '确认删除节点',
    description: nodeName => `将永久删除节点「${nodeName}」及其配置与相关监控数据，操作无法从面板恢复。`,
  },
}

export function matchesNodeFilter(node: ManagedNode, filter: NodeListFilter): boolean {
  if (filter === 'hidden')
    return Boolean(node.hidden)
  if (filter === 'disabled')
    return Boolean(node.disabled)
  if (filter === 'provider')
    return node.traffic_source === 'provider'
  return true
}

export function matchesNodeSearch(node: ManagedNode, rawQuery: string): boolean {
  const query = rawQuery.trim().toLowerCase()
  if (!query)
    return true
  if (node.id.toLowerCase().startsWith(query))
    return true
  return [
    node.name,
    node.group,
    node.region,
    node.public_remark,
  ].some(field => (field ?? '').toLowerCase().includes(query))
}

export function filterManagedNodes(
  nodes: ManagedNode[],
  query: string,
  filter: NodeListFilter,
): ManagedNode[] {
  return nodes.filter(node => matchesNodeFilter(node, filter) && matchesNodeSearch(node, query))
}

export function resolveNodeSelection(context: NodeSelectionContext): NodeSelectionResolution {
  const requestedId = context.queryNodeId?.trim() ?? ''
  if (requestedId) {
    const match = context.nodes.find(node => node.id === requestedId)
    if (match) {
      return {
        mode: 'selected',
        node: match,
      }
    }
    return {
      mode: 'missing',
      requestedId,
    }
  }
  if (context.nodes.length === 0)
    return { mode: 'empty' }
  const pinnedId = context.pinnedNodeId?.trim() ?? ''
  if (pinnedId) {
    const pinnedMatch = context.nodes.find(node => node.id === pinnedId)
    if (pinnedMatch) {
      return {
        mode: 'selected',
        node: pinnedMatch,
      }
    }
  }
  if (context.isDesktop) {
    return {
      mode: 'selected',
      node: context.nodes[0]!,
    }
  }
  return { mode: 'list' }
}
