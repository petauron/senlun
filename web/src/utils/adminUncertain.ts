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

// Session-scoped in-memory metadata only (never stores tokens, passwords, form drafts, or localStorage).
export const enrollUncertainLock = ref<string | null>(null)
export const nodeDangerUncertainLocks = ref<Record<string, PendingUncertainDangerLock>>({})
export const trafficRemoveUncertainLocks = ref<Record<string, PendingUncertainTrafficRemoveLock>>({})

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

export function clearAdminUncertainLocks(): void {
  enrollUncertainLock.value = null
  nodeDangerUncertainLocks.value = {}
  trafficRemoveUncertainLocks.value = {}
}
