<script setup lang="ts">
import type { RouteLocationNormalized } from 'vue-router'
import type { ManagedNode, ProbeTask } from '@/utils/admin'
import type { NodeTargetScopeSummary } from '@/utils/adminEditing'
import type { PendingUncertainConfigDeleteLock } from '@/utils/adminUncertain'
import { computed, inject, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import ProbeHistory from '@/components/ProbeHistory.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import SelectControl from '@/components/ui/SelectControl.vue'
import { useAdminAction } from '@/composables/useAdminAction'
import { ADMIN_DRAFT_REGISTRY_KEY, useAdminDraftGuard } from '@/composables/useAdminDraftGuard'
import { useAuthStore } from '@/stores/auth'
import {
  buildAssignableNodes,
  cloneProbeTask,
  createClientUuid,
  isProbeDraftDirty,
  requiresGlobalScopeConfirmation,
  scrollAndFocusHeading,
  shouldGuardGlobalScope,
  summarizeNodeTargetScope,
  validateScopedNodeSelection,
} from '@/utils/adminEditing'
import {
  isSameActiveSession,
  probeDeleteUncertainLocks,
  setProbeDeleteUncertainLock,
} from '@/utils/adminUncertain'
import { ApiError, getSharedApi } from '@/utils/api'
import { getSessionGeneration } from '@/utils/session'

interface PendingUncertainProbeSave {
  targetId: string
  wasExisting: boolean
  name: string
  submitted: ProbeTask
  revision: number
  message: string
  resyncError: string
}

interface PendingProbeRemoveTarget {
  id: string
  name: string
}

const props = defineProps<{ probes: ProbeTask[], nodes: ManagedNode[], refresh: () => Promise<void> }>()
const emit = defineEmits<{ probeDeleted: [id: string] }>()
const auth = useAuthStore()
const draftRegistry = inject(ADMIN_DRAFT_REGISTRY_KEY, null)

function blank(): ProbeTask {
  return {
    id: '',
    name: '',
    weight: 0,
    kind: 'icmp',
    target: '',
    interval_seconds: 60,
    timeout_seconds: 5,
    enabled: true,
    node_ids: [],
  }
}

const draft = ref<ProbeTask>(blank())
const baseline = ref<ProbeTask>(blank())
const pendingCreateId = ref('')
const seenPendingCreateId = ref('')
const removedTaskName = ref('')
const unverifiedScopedTarget = ref<{ id: string, nodeIds: string[], enabled: boolean } | null>(null)
const confirmGlobalScope = ref(false)
const editRevision = ref(0)
const refreshBusy = ref(false)
const removingTarget = ref<PendingProbeRemoveTarget | null>(null)
const formHeadingRef = ref<HTMLElement | null>(null)
const pendingUncertainSave = ref<PendingUncertainProbeSave | null>(null)
const saveValidationError = ref('')
const pendingDeleteLocks = probeDeleteUncertainLocks

const saveAction = useAdminAction(props.refresh)
const deleteAction = useAdminAction(props.refresh)

const {
  busy: saveBusy,
  error: saveError,
  resyncError: saveResyncError,
  success: saveSuccess,
  needsResync: saveNeedsResync,
} = saveAction
const {
  busy: deleteBusy,
  error: deleteError,
  resyncError: deleteResyncError,
  success: deleteSuccess,
  outcome: deleteOutcome,
  needsResync: deleteNeedsResync,
} = deleteAction

const busy = computed(() => saveBusy.value || deleteBusy.value || refreshBusy.value)
const hasUnsavedChanges = computed(() => isProbeDraftDirty(draft.value, baseline.value, confirmGlobalScope.value))
const effectiveEditingId = computed(() => draft.value.id || pendingCreateId.value)
const liveEditingTask = computed(() => effectiveEditingId.value
  ? props.probes.find(task => task.id === effectiveEditingId.value)
  : undefined)
const editingTaskMissing = computed(() => Boolean(draft.value.id) && !props.probes.some(task => task.id === draft.value.id))

// Combine latest GET snapshot with any still-unverified scoped submission for the same ID so neither stale GET nor 400 retries can bypass #312.
const matchingUnverifiedScope = computed(() => {
  const pending = unverifiedScopedTarget.value
  if (!pending || !effectiveEditingId.value || pending.id !== effectiveEditingId.value)
    return null
  return pending
})
const persistedBaseScope = computed(() => {
  if (liveEditingTask.value)
    return liveEditingTask.value
  if (draft.value.id)
    return baseline.value
  return undefined
})
const persistedNodeIds = computed(() => {
  const ids = new Set<string>()
  if (persistedBaseScope.value) {
    for (const id of persistedBaseScope.value.node_ids)
      ids.add(id)
  }
  if (matchingUnverifiedScope.value) {
    for (const id of matchingUnverifiedScope.value.nodeIds)
      ids.add(id)
  }
  return Array.from(ids)
})
const guardGlobalScope = computed(() => {
  if (persistedBaseScope.value && shouldGuardGlobalScope(persistedBaseScope.value))
    return true
  if (matchingUnverifiedScope.value)
    return true
  return false
})
const persistedScopeSignature = computed(() => {
  const basePart = persistedBaseScope.value
    ? `${persistedBaseScope.value.id}|${persistedBaseScope.value.enabled ? '1' : '0'}|${[...persistedBaseScope.value.node_ids].sort().join(',')}`
    : ''
  const unverifiedPart = matchingUnverifiedScope.value
    ? `${matchingUnverifiedScope.value.id}|${matchingUnverifiedScope.value.enabled ? '1' : '0'}|${[...matchingUnverifiedScope.value.nodeIds].sort().join(',')}`
    : ''
  return `${basePart}::${unverifiedPart}`
})

const visibleNodes = computed(() => props.nodes.filter(node => !node.hidden && !node.disabled))
const assignableNodes = computed(() => buildAssignableNodes(props.nodes, draft.value.node_ids, persistedNodeIds.value))
const needsGlobalConfirmation = computed(() => requiresGlobalScopeConfirmation(
  guardGlobalScope.value,
  draft.value.node_ids,
  draft.value.enabled,
))
const resultNode = ref(visibleNodes.value[0]?.id ?? '')

const activeDeleteLocks = computed(() => Object.values(pendingDeleteLocks.value))
const activeRemovingUncertain = computed(() => {
  const target = removingTarget.value
  if (!target)
    return null
  return pendingDeleteLocks.value[`probes:${target.id}`] ?? null
})
const removingTargetInvalidReason = computed(() => {
  const target = removingTarget.value
  if (!target)
    return ''
  if (!props.probes.some(task => task.id === target.id))
    return `探测任务「${target.name}」在最新同步中已不存在，本次确认已失效，无法执行。`
  return ''
})
const removingDiscardsActiveDraft = computed(() => Boolean(
  removingTarget.value
  && effectiveEditingId.value === removingTarget.value.id
  && hasUnsavedChanges.value,
))
const saveAlert = computed(() => {
  const pending = pendingUncertainSave.value
  if (pending) {
    return pending.resyncError
      ? `${pending.message}（重新同步失败：${pending.resyncError}）`
      : pending.message
  }
  return saveError.value
})
const showSaveResync = computed(() => saveNeedsResync.value || Boolean(pendingUncertainSave.value))
const showListDeleteRefreshAlert = computed(() => removingTarget.value === null && deleteOutcome.value === 'refresh_failed' && Boolean(deleteError.value))
const targetPlaceholder = computed(() => {
  if (draft.value.kind === 'http')
    return 'https://example.com'
  if (draft.value.kind === 'tcp')
    return 'example.com:443'
  return 'example.com'
})

function formatPendingAlert(message: string, resyncFailure: string): string {
  return resyncFailure
    ? `${message}（重新同步失败：${resyncFailure}）`
    : message
}

function scopeSummary(task: ProbeTask): NodeTargetScopeSummary {
  return summarizeNodeTargetScope(task.node_ids, props.nodes)
}

function focusFormHeading(): void {
  void nextTick(() => scrollAndFocusHeading(formHeadingRef.value))
}

function applyResetDraft(clearFeedback = true): void {
  draft.value = blank()
  baseline.value = blank()
  pendingCreateId.value = ''
  seenPendingCreateId.value = ''
  removedTaskName.value = ''
  unverifiedScopedTarget.value = null
  confirmGlobalScope.value = false
  pendingUncertainSave.value = null
  saveValidationError.value = ''
  if (clearFeedback)
    saveAction.clearFeedback()
}

function applyEditTask(task: ProbeTask, clearFeedback = true): void {
  draft.value = cloneProbeTask(task)
  baseline.value = cloneProbeTask(task)
  pendingCreateId.value = ''
  seenPendingCreateId.value = ''
  removedTaskName.value = ''
  unverifiedScopedTarget.value = null
  confirmGlobalScope.value = false
  pendingUncertainSave.value = null
  saveValidationError.value = ''
  if (clearFeedback)
    saveAction.clearFeedback()
}

function markPendingCreateRemoved(removedId: string, displayName: string): void {
  if (!draft.value.id && pendingCreateId.value === removedId) {
    draft.value.id = removedId
    removedTaskName.value = displayName
    pendingCreateId.value = ''
    seenPendingCreateId.value = ''
    unverifiedScopedTarget.value = null
    pendingUncertainSave.value = null
  }
}

watch(visibleNodes, (nodes) => {
  if (!nodes.some(node => node.id === resultNode.value))
    resultNode.value = nodes[0]?.id ?? ''
})

watch(() => draft.value.node_ids.length, (count) => {
  if (count > 0)
    confirmGlobalScope.value = false
})

watch(persistedScopeSignature, (next, prev) => {
  if (next !== prev)
    confirmGlobalScope.value = false
})

watch(
  [
    draft,
    confirmGlobalScope,
  ],
  () => {
    if (!refreshBusy.value)
      editRevision.value++
  },
  { deep: true, flush: 'sync' },
)

watch(
  () => props.probes,
  (probes) => {
    if (!draft.value.id) {
      if (pendingCreateId.value) {
        const created = probes.find(task => task.id === pendingCreateId.value)
        if (created) {
          seenPendingCreateId.value = pendingCreateId.value
        }
        else if (seenPendingCreateId.value === pendingCreateId.value) {
          if (!hasUnsavedChanges.value)
            applyResetDraft(false)
          else
            markPendingCreateRemoved(pendingCreateId.value, draft.value.name || pendingCreateId.value)
        }
      }
      return
    }
    const live = probes.find(task => task.id === draft.value.id)
    if (!live) {
      if (!hasUnsavedChanges.value)
        applyResetDraft(false)
      return
    }
    if (!hasUnsavedChanges.value) {
      applyEditTask(live, false)
      return
    }
    if (!isProbeDraftDirty(draft.value, live, false))
      baseline.value = cloneProbeTask(live)
  },
)

function isStayingOnProbesSection(to: RouteLocationNormalized): boolean {
  return to.name === 'admin' && to.query.section === 'probes'
}

const draftGuard = useAdminDraftGuard({
  isLoggedIn: () => auth.loggedIn,
  isDirty: () => hasUnsavedChanges.value,
  isBusy: () => busy.value,
  isStayingInSameContext: isStayingOnProbesSection,
})

const {
  pending: draftGuardPending,
  isOpen: draftGuardOpen,
  cancel: cancelDraftGuard,
  confirm: confirmDraftGuard,
  restoreFocus: restoreDraftGuardFocus,
} = draftGuard

const draftGuardSwitchTarget = computed(() => {
  const action = draftGuardPending.value
  if (action?.kind === 'switch')
    return action.targetLabel
  return ''
})

const draftGuardConfirmLabel = computed(() => {
  const action = draftGuardPending.value
  if (!action)
    return '放弃修改并离开'
  if (action.kind === 'refresh')
    return '放弃修改并刷新'
  if (action.kind === 'switch')
    return '放弃修改并切换'
  return '放弃修改并离开'
})

const unregisterDraft = draftRegistry?.register({
  section: 'probes',
  isDirty: () => hasUnsavedChanges.value,
  isBusy: () => busy.value,
  requestRefresh: triggerEl => draftGuard.requestRefresh(async () => {
    const confirmedId = draft.value.id
    const confirmedRevision = editRevision.value
    refreshBusy.value = true
    saveAction.clearFeedback()
    try {
      await props.refresh()
      if (draft.value.id !== confirmedId || editRevision.value !== confirmedRevision)
        return
      pendingUncertainSave.value = null
      const live = confirmedId
        ? props.probes.find(task => task.id === confirmedId)
        : undefined
      if (live)
        applyEditTask(live, false)
      else
        applyResetDraft(false)
    }
    catch (cause) {
      saveError.value = cause instanceof Error
        ? cause.message
        : String(cause)
    }
    finally {
      refreshBusy.value = false
    }
  }, triggerEl),
})

onBeforeUnmount(() => {
  unregisterDraft?.()
})

function editTask(task: ProbeTask, event?: MouseEvent): void {
  if (busy.value)
    return
  const triggerEl = event?.currentTarget instanceof HTMLElement
    ? event.currentTarget
    : null
  if (triggerEl)
    triggerEl.focus()
  if (draft.value.id === task.id) {
    focusFormHeading()
    return
  }
  draftGuard.requestSwitch(
    task.id,
    `探测任务「${task.name}」`,
    () => applyEditTask(task),
    triggerEl,
    focusFormHeading,
  )
}

function cancelEdit(event?: MouseEvent): void {
  if (busy.value)
    return
  const triggerEl = event?.currentTarget instanceof HTMLElement
    ? event.currentTarget
    : null
  if (triggerEl)
    triggerEl.focus()
  draftGuard.requestSwitch(
    '__new__',
    '新增探测任务',
    () => applyResetDraft(true),
    triggerEl,
  )
}

function openRemove(task: ProbeTask): void {
  if (busy.value)
    return
  if (deleteOutcome.value === 'failed' || deleteOutcome.value === 'saved')
    deleteAction.clearFeedback()
  removingTarget.value = {
    id: task.id,
    name: task.name,
  }
}

function closeRemove(): void {
  if (busy.value)
    return
  removingTarget.value = null
  if (deleteOutcome.value === 'failed')
    deleteAction.clearFeedback()
}

function unlockUncertainDelete(key: string): void {
  if (!pendingDeleteLocks.value[key])
    return
  const next = { ...pendingDeleteLocks.value }
  delete next[key]
  pendingDeleteLocks.value = next
  if (deleteOutcome.value === 'uncertain')
    deleteAction.clearFeedback()
}

async function save(): Promise<void> {
  if (editingTaskMissing.value)
    return
  saveValidationError.value = ''
  const scopeError = validateScopedNodeSelection({
    guardGlobalScope: guardGlobalScope.value,
    selectedIds: draft.value.node_ids,
    enabled: draft.value.enabled,
    confirmGlobalScope: confirmGlobalScope.value,
    nodes: props.nodes,
  })
  if (scopeError) {
    saveValidationError.value = scopeError
    saveSuccess.value = ''
    return
  }
  const wasExisting = Boolean(draft.value.id)
  if (!wasExisting && !pendingCreateId.value)
    pendingCreateId.value = createClientUuid()
  const targetId = wasExisting
    ? draft.value.id
    : pendingCreateId.value
  const submitted = cloneProbeTask({
    ...draft.value,
    id: targetId,
  })
  const submittedRevision = editRevision.value
  pendingUncertainSave.value = null
  const result = await saveAction.run(async () => {
    await getSharedApi().post('admin/probes', submitted)
  }, {
    message: '已保存探测任务',
    onCommitted: () => {
      applyResetDraft(false)
    },
  })
  if (result === 'uncertain') {
    if (shouldGuardGlobalScope(submitted)) {
      unverifiedScopedTarget.value = {
        id: targetId,
        nodeIds: [...submitted.node_ids],
        enabled: Boolean(submitted.enabled),
      }
    }
    pendingUncertainSave.value = {
      targetId,
      wasExisting,
      name: submitted.name,
      submitted,
      revision: submittedRevision,
      message: saveError.value,
      resyncError: '',
    }
  }
}

async function resyncSave(): Promise<void> {
  saveValidationError.value = ''
  const pending = pendingUncertainSave.value
  if (!pending) {
    await saveAction.resync('已重新同步最新状态')
    return
  }
  pending.resyncError = ''
  const ok = await saveAction.resync({
    verify: () => {
      pendingUncertainSave.value = null
      const fresh = props.probes.find(task => task.id === pending.targetId)
      if (fresh && shouldGuardGlobalScope(fresh) && unverifiedScopedTarget.value?.id === pending.targetId)
        unverifiedScopedTarget.value = null
      if (!fresh) {
        return {
          resolved: true,
          message: pending.wasExisting
            ? `已核实：探测任务「${pending.name}」已不存在`
            : `已同步最新状态：列表中尚未出现新探测任务「${pending.name}」，已保留当前输入与同一标识，请核对后再决定是否重新保存。`,
        }
      }
      const applied = !isProbeDraftDirty(pending.submitted, fresh, false)
      const sameDraftContext = pending.wasExisting
        ? draft.value.id === pending.targetId
        : !draft.value.id && pendingCreateId.value === pending.targetId
      const editedAfterSubmit = sameDraftContext && editRevision.value !== pending.revision
      if (applied) {
        if (sameDraftContext) {
          if (editedAfterSubmit) {
            if (!pending.wasExisting) {
              draft.value.id = fresh.id
              pendingCreateId.value = ''
              seenPendingCreateId.value = ''
            }
            baseline.value = cloneProbeTask(fresh)
          }
          else if (pending.wasExisting) {
            applyEditTask(fresh, false)
          }
          else {
            applyResetDraft(false)
          }
        }
        if (pending.wasExisting) {
          return {
            resolved: true,
            message: editedAfterSubmit
              ? `已核实：上次提交的探测任务「${pending.name}」修改已在服务端生效；您之后的新修改已保留，尚未保存。`
              : `已核实：探测任务「${pending.name}」的修改已在服务端生效`,
          }
        }
        return {
          resolved: true,
          message: editedAfterSubmit
            ? `已核实：新探测任务「${pending.name}」已创建；您之后的新修改已保留，尚未保存。`
            : `已核实：新探测任务「${pending.name}」已在服务端创建`,
        }
      }
      if (sameDraftContext && !pending.wasExisting) {
        draft.value.id = fresh.id
        pendingCreateId.value = ''
        seenPendingCreateId.value = ''
        baseline.value = cloneProbeTask(fresh)
      }
      return {
        resolved: true,
        message: `已同步最新状态：服务端探测任务「${pending.name}」与刚才提交不一致，已保留当前输入，请核对后再决定是否重新保存。`,
      }
    },
  })
  if (!ok && pendingUncertainSave.value)
    pendingUncertainSave.value.resyncError = saveResyncError.value
}

async function remove(): Promise<void> {
  if (!removingTarget.value || activeRemovingUncertain.value || removingTargetInvalidReason.value)
    return
  const sessionGen = getSessionGeneration()
  const { id, name } = removingTarget.value
  const key = `probes:${id}`
  let committedDelete = false
  const result = await deleteAction.run(async () => {
    await getSharedApi().post(`admin/probes/${encodeURIComponent(id)}/delete`)
  }, {
    message: `已删除探测任务「${name}」`,
    riskyWrite: true,
    // Backend delete_monitoring_config commits probe deletion before optional metrics cleanup, which can return structured 503.
    isUncertainError: cause => cause instanceof ApiError && cause.statusCode === 503,
    uncertainMessage: detail => `删除探测任务「${name}」结果未确认（${detail}）。请先重新同步核对当前列表，勿直接重复提交。`,
    onCommitted: () => {
      if (!isSameActiveSession(sessionGen, auth.loggedIn))
        return
      removingTarget.value = null
      unlockUncertainDelete(key)
      committedDelete = true
    },
  })
  if (committedDelete && isSameActiveSession(sessionGen, auth.loggedIn)) {
    if (effectiveEditingId.value === id)
      applyResetDraft(false)
    emit('probeDeleted', id)
    return
  }
  if (result === 'uncertain') {
    setProbeDeleteUncertainLock(sessionGen, auth.loggedIn, {
      key,
      kind: 'probes',
      id,
      name,
      message: deleteError.value,
      resyncError: '',
    })
  }
}

async function resyncDelete(entry?: PendingUncertainConfigDeleteLock<'probes'>): Promise<void> {
  if (!entry) {
    await deleteAction.resync('已重新同步最新状态')
    return
  }
  const sessionGen = getSessionGeneration()
  const targetWasDirtyBeforeResync = effectiveEditingId.value === entry.id && hasUnsavedChanges.value
  entry.resyncError = ''
  let verifiedDeleted = false
  const ok = await deleteAction.resync({
    verify: () => {
      if (!isSameActiveSession(sessionGen, auth.loggedIn)) {
        return {
          resolved: false,
          error: '',
        }
      }
      unlockUncertainDelete(entry.key)
      const exists = props.probes.some(task => task.id === entry.id)
      if (!exists) {
        if (removingTarget.value?.id === entry.id)
          removingTarget.value = null
        verifiedDeleted = true
        return {
          resolved: true,
          message: `已核实：探测任务「${entry.name}」已删除`,
        }
      }
      return {
        resolved: true,
        message: `已核对最新状态：探测任务「${entry.name}」仍存在，可确认后再决定是否重试删除。`,
      }
    },
  })
  if (verifiedDeleted && isSameActiveSession(sessionGen, auth.loggedIn)) {
    // Read-only resync must preserve dirty user edits (surfaced via editingTaskMissing) and block old UUID resurrection.
    if (effectiveEditingId.value === entry.id) {
      if (!targetWasDirtyBeforeResync && !hasUnsavedChanges.value)
        applyResetDraft(false)
      else
        markPendingCreateRemoved(entry.id, entry.name)
    }
    return
  }
  if (!ok && deleteResyncError.value && isSameActiveSession(sessionGen, auth.loggedIn) && pendingDeleteLocks.value[entry.key])
    pendingDeleteLocks.value[entry.key]!.resyncError = deleteResyncError.value
}
</script>

<template>
  <div class="space-y-4">
    <CardX title="探测任务" content-class="space-y-4">
      <div v-if="showListDeleteRefreshAlert" class="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/40 bg-card p-3">
        <p role="alert" class="text-sm text-destructive">
          {{ deleteError }}
        </p>
        <Button v-if="deleteNeedsResync" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncDelete()">
          重新同步
        </Button>
      </div>
      <div v-if="removingTarget === null && activeDeleteLocks.length" class="space-y-2">
        <div v-for="entry in activeDeleteLocks" :key="entry.key" class="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/40 p-3">
          <p role="alert" class="text-sm text-destructive">
            {{ formatPendingAlert(entry.message, entry.resyncError) }}
          </p>
          <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncDelete(entry)">
            重新同步
          </Button>
        </div>
      </div>
      <p v-if="removingTarget === null && deleteSuccess" role="status" class="text-sm text-success">
        {{ deleteSuccess }}
      </p>
      <p v-if="!probes.length" class="text-sm text-muted-foreground">
        尚无探测任务。Agent 会按配置主动探测并上报结果。
      </p>
      <ul v-else aria-label="探测任务列表" class="space-y-2">
        <li
          v-for="task in probes"
          :key="task.id"
          :data-editing="draft.id === task.id ? 'true' : undefined"
          class="flex flex-wrap items-center justify-between gap-3 rounded-lg border p-3 transition-colors"
          :class="draft.id === task.id ? 'border-primary bg-primary/10' : 'border-border/80'"
        >
          <div class="min-w-0 flex-1 space-y-1">
            <div class="flex flex-wrap items-center gap-1.5">
              <p class="font-medium break-words">
                {{ task.name }}
              </p>
              <Badge :variant="task.enabled ? 'secondary' : 'outline'">
                {{ task.enabled ? '启用' : '停用' }}
              </Badge>
              <Badge v-if="draft.id === task.id" variant="default">
                正在编辑
              </Badge>
              <Badge v-if="draft.id === task.id && hasUnsavedChanges" variant="outline">
                未保存
              </Badge>
            </div>
            <p class="break-all text-sm text-muted-foreground">
              {{ task.kind.toUpperCase() }} · {{ task.target }} · 每 {{ task.interval_seconds }} 秒 · 顺序 {{ task.weight }}
            </p>
            <div class="flex flex-wrap items-center gap-1.5 text-xs">
              <Badge variant="outline">
                {{ scopeSummary(task).label }}
              </Badge>
              <Badge v-if="scopeSummary(task).warningLabel" variant="destructive">
                {{ scopeSummary(task).warningLabel }}
              </Badge>
            </div>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="editTask(task, $event)">
              编辑
            </Button>
            <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="openRemove(task)">
              删除
            </Button>
          </div>
        </li>
      </ul>
    </CardX>

    <CardX content-class="space-y-4">
      <template #header>
        <div class="flex flex-wrap items-center gap-2">
          <h2 ref="formHeadingRef" tabindex="-1" class="font-medium outline-none">
            {{ draft.id ? '编辑探测任务' : '新增探测任务' }}
          </h2>
          <Badge v-if="draft.id && (baseline.name || removedTaskName)" variant="secondary">
            {{ baseline.name || removedTaskName }}
          </Badge>
          <Badge v-if="hasUnsavedChanges" variant="outline">
            未保存
          </Badge>
        </div>
      </template>
      <p v-if="editingTaskMissing" role="alert" class="rounded-md border border-destructive/40 p-3 text-sm text-destructive">
        当前正在编辑的探测任务「{{ baseline.name || removedTaskName || draft.id }}」在最新同步中已不存在，无法继续保存。当前未保存输入仍保留在表单中供核对或复制，请点击“取消编辑”返回新增态。
      </p>
      <p v-else-if="hasUnsavedChanges" role="status" class="text-sm text-muted-foreground">
        有未保存修改；切换编辑对象、切换栏目或主动刷新前会提示确认。
      </p>
      <form class="space-y-4" :aria-busy="busy" @submit.prevent="save">
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <label class="grid gap-2 text-sm">名称<Input v-model="draft.name" class="h-11" required maxlength="128" /></label>
          <label class="grid gap-2 text-sm">显示顺序<Input v-model.number="draft.weight" class="h-11" type="number" min="-100000" max="100000" step="1" required /></label>
          <label class="grid gap-2 text-sm">类型<SelectControl v-model="draft.kind" aria-label="类型" :disabled="busy" :options="[{ value: 'icmp', label: 'ICMP' }, { value: 'tcp', label: 'TCP' }, { value: 'http', label: 'HTTP / HTTPS' }]" /></label>
          <label class="grid gap-2 text-sm">目标<Input v-model="draft.target" class="h-11" :placeholder="targetPlaceholder" maxlength="2048" required /></label>
          <label class="grid gap-2 text-sm">间隔（秒）<Input v-model="draft.interval_seconds" class="h-11" type="number" min="5" max="3600" required /></label>
          <label class="grid gap-2 text-sm">超时（秒）<Input v-model="draft.timeout_seconds" class="h-11" type="number" min="1" :max="Math.min(30, draft.interval_seconds)" required /></label>
          <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="draft.enabled" type="checkbox" class="size-4 accent-primary">启用任务</label>
        </fieldset>
        <p class="text-xs text-muted-foreground">
          显示顺序越小越靠前；相同数值按创建顺序排列，探测任务本身不受影响。
        </p>
        <fieldset :disabled="busy" class="space-y-2">
          <legend class="mb-2 text-sm font-medium">
            执行节点
          </legend>
          <div class="grid max-h-64 gap-2 overflow-auto rounded-md border p-3 sm:grid-cols-2 lg:grid-cols-3">
            <label v-for="node in assignableNodes" :key="node.id" class="flex min-h-9 items-center gap-2 text-sm" :class="node.revoked || node.missing ? 'text-destructive' : ''"><input v-model="draft.node_ids" type="checkbox" :value="node.id" class="size-4 accent-primary">{{ node.label }}</label>
          </div>
          <p class="text-xs text-muted-foreground">
            不选择时应用于所有活动节点，包括之后注册的节点。
          </p>
          <div v-if="needsGlobalConfirmation" class="space-y-2 rounded-md border border-destructive/40 p-3 text-xs">
            <p class="text-destructive">
              当前未指定任何节点且将启用此任务。为避免静默扩大探测范围，请选择活动节点、保持停用，或显式确认改为应用于所有活动节点。
            </p>
            <label class="flex min-h-9 items-center gap-2 text-sm"><input v-model="confirmGlobalScope" type="checkbox" class="size-4 accent-primary">确认改为应用于所有活动节点</label>
          </div>
        </fieldset>
        <div class="space-y-2">
          <div class="flex flex-wrap items-center gap-2">
            <Button type="submit" class="min-h-11" :disabled="busy || editingTaskMissing">
              {{ saveBusy ? '正在保存…' : '保存任务' }}
            </Button>
            <Button v-if="showSaveResync" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncSave">
              重新同步
            </Button>
            <Button v-if="draft.id" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="cancelEdit($event)">
              取消编辑
            </Button>
          </div>
          <p v-if="saveValidationError" role="alert" class="text-sm text-destructive">
            {{ saveValidationError }}
          </p>
          <p v-if="saveAlert" role="alert" class="text-sm text-destructive">
            {{ saveAlert }}
          </p>
          <p v-if="saveSuccess" role="status" class="text-sm text-success">
            {{ saveSuccess }}
          </p>
        </div>
      </form>
    </CardX>

    <CardX v-if="visibleNodes.length" title="探测结果">
      <label for="probe-result-node" class="grid gap-2 text-sm">选择节点<SelectControl id="probe-result-node" v-model="resultNode" aria-label="选择节点" :options="visibleNodes.map(node => ({ value: node.id, label: node.name }))" /></label>
    </CardX>
    <ProbeHistory v-if="resultNode" :uuid="resultNode" />

    <Dialog :open="draftGuardOpen" @update:open="value => { if (!value) cancelDraftGuard() }">
      <DialogContent @close-auto-focus="restoreDraftGuardFocus">
        <DialogHeader>
          <DialogTitle>放弃未保存修改？</DialogTitle>
          <DialogDescription v-if="draftGuardSwitchTarget">
            切换到 {{ draftGuardSwitchTarget }} 会丢弃当前探测任务表单未保存的修改，不会保存任何更改。
          </DialogDescription>
          <DialogDescription v-else-if="draftGuardPending?.kind === 'refresh'">
            刷新管理数据会丢弃当前探测任务表单未保存的修改，不会保存任何更改。
          </DialogDescription>
          <DialogDescription v-else>
            离开探测栏目会丢弃当前探测任务表单未保存的修改，不会保存任何更改。
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" class="min-h-11" @click="cancelDraftGuard">
            取消，继续编辑
          </Button>
          <Button variant="destructive" class="min-h-11" @click="confirmDraftGuard">
            {{ draftGuardConfirmLabel }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog :open="removingTarget !== null" @update:open="value => { if (!value) closeRemove() }">
      <DialogContent :show-close="!busy" @interact-outside="event => { if (busy) event.preventDefault() }" @escape-key-down="event => { if (busy) event.preventDefault() }">
        <DialogHeader>
          <DialogTitle>删除探测任务</DialogTitle>
          <DialogDescription>确认删除“{{ removingTarget?.name }}”？Agent 将停止执行此任务。</DialogDescription>
        </DialogHeader>
        <p v-if="removingDiscardsActiveDraft && !removingTargetInvalidReason" class="text-sm text-muted-foreground">
          当前表单中针对此探测任务的未保存修改也会一并丢弃。
        </p>
        <p v-if="removingTargetInvalidReason" role="alert" class="text-sm text-destructive">
          {{ removingTargetInvalidReason }}
        </p>
        <p v-else-if="activeRemovingUncertain" role="alert" class="text-sm text-destructive">
          {{ formatPendingAlert(activeRemovingUncertain.message, activeRemovingUncertain.resyncError) }}
        </p>
        <p v-else-if="deleteError" role="alert" class="text-sm text-destructive">
          {{ deleteError }}
        </p>
        <p v-if="!removingTargetInvalidReason && !activeRemovingUncertain && deleteSuccess" role="status" class="text-sm text-success">
          {{ deleteSuccess }}
        </p>
        <DialogFooter>
          <Button variant="outline" class="min-h-11" :disabled="busy" @click="closeRemove">
            取消
          </Button>
          <Button v-if="activeRemovingUncertain && !removingTargetInvalidReason" variant="outline" class="min-h-11" :disabled="busy" @click="resyncDelete(activeRemovingUncertain)">
            重新同步
          </Button>
          <Button variant="destructive" class="min-h-11" :disabled="busy || Boolean(activeRemovingUncertain) || Boolean(removingTargetInvalidReason)" @click="remove">
            {{ deleteBusy ? '正在删除…' : '确认删除探测任务' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
