import { ref } from 'vue'

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

export function clearAdminUncertainLocks(): void {
  enrollUncertainLock.value = null
  nodeDangerUncertainLocks.value = {}
  trafficRemoveUncertainLocks.value = {}
}
