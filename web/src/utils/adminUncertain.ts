import type { NodeListFilter } from './adminNodes'
import { ref } from 'vue'
import { getSessionGeneration } from './session'

export type NodeDangerActionKind = 'rotate' | 'revoke' | 'delete' | 'reset-traffic'

export interface PendingUncertainDangerLock {
  key: string
  action: NodeDangerActionKind
  nodeId: string
  nodeName: string
  verifiable: boolean
  message: string
  resyncError: string
}

export interface PendingUncertainTrafficRemoveLock {
  nodeId: string
  nodeName: string
  message: string
  resyncError: string
}

export type AlertConfigDeleteKind = 'channels' | 'alert-rules'
export type MonitoringConfigDeleteKind = 'probes' | AlertConfigDeleteKind

export interface PendingUncertainConfigDeleteLock<K extends MonitoringConfigDeleteKind = MonitoringConfigDeleteKind> {
  key: string
  kind: K
  id: string
  name: string
  message: string
  resyncError: string
}

export interface AdminNodesViewState {
  searchQuery: string
  statusFilter: NodeListFilter
  listScrollY: number | null
  listScrollTop: number | null
  lastNodeId: string | null
}

function defaultAdminNodesViewState(): AdminNodesViewState {
  return {
    searchQuery: '',
    statusFilter: 'all',
    listScrollY: null,
    listScrollTop: null,
    lastNodeId: null,
  }
}

// Session-scoped in-memory metadata only (never stores tokens, passwords, webhook URLs, form drafts, or localStorage).
export const enrollUncertainLock = ref<string | null>(null)
export const nodeDangerUncertainLocks = ref<Record<string, PendingUncertainDangerLock>>({})
export const trafficRemoveUncertainLocks = ref<Record<string, PendingUncertainTrafficRemoveLock>>({})
export const probeDeleteUncertainLocks = ref<Record<string, PendingUncertainConfigDeleteLock<'probes'>>>({})
export const alertConfigDeleteUncertainLocks = ref<Record<string, PendingUncertainConfigDeleteLock<AlertConfigDeleteKind>>>({})
export const adminNodesViewState = ref<AdminNodesViewState>(defaultAdminNodesViewState())

export function isSameActiveSession(startGeneration: number, loggedIn: boolean): boolean {
  return loggedIn && getSessionGeneration() === startGeneration
}

export function setEnrollUncertainLock(startGeneration: number, loggedIn: boolean, message: string): boolean {
  if (!isSameActiveSession(startGeneration, loggedIn))
    return false
  enrollUncertainLock.value = message
  return true
}

export function setNodeDangerUncertainLock(
  startGeneration: number,
  loggedIn: boolean,
  entry: PendingUncertainDangerLock,
): boolean {
  if (!isSameActiveSession(startGeneration, loggedIn))
    return false
  nodeDangerUncertainLocks.value = {
    ...nodeDangerUncertainLocks.value,
    [entry.key]: entry,
  }
  return true
}

export function setTrafficRemoveUncertainLock(
  startGeneration: number,
  loggedIn: boolean,
  entry: PendingUncertainTrafficRemoveLock,
): boolean {
  if (!isSameActiveSession(startGeneration, loggedIn))
    return false
  trafficRemoveUncertainLocks.value = {
    ...trafficRemoveUncertainLocks.value,
    [entry.nodeId]: entry,
  }
  return true
}

export function setProbeDeleteUncertainLock(
  startGeneration: number,
  loggedIn: boolean,
  entry: PendingUncertainConfigDeleteLock<'probes'>,
): boolean {
  if (!isSameActiveSession(startGeneration, loggedIn))
    return false
  probeDeleteUncertainLocks.value = {
    ...probeDeleteUncertainLocks.value,
    [entry.key]: entry,
  }
  return true
}

export function setAlertConfigDeleteUncertainLock(
  startGeneration: number,
  loggedIn: boolean,
  entry: PendingUncertainConfigDeleteLock<AlertConfigDeleteKind>,
): boolean {
  if (!isSameActiveSession(startGeneration, loggedIn))
    return false
  alertConfigDeleteUncertainLocks.value = {
    ...alertConfigDeleteUncertainLocks.value,
    [entry.key]: entry,
  }
  return true
}

export function clearAdminUncertainLocks(): void {
  enrollUncertainLock.value = null
  nodeDangerUncertainLocks.value = {}
  trafficRemoveUncertainLocks.value = {}
  probeDeleteUncertainLocks.value = {}
  alertConfigDeleteUncertainLocks.value = {}
  adminNodesViewState.value = defaultAdminNodesViewState()
}
