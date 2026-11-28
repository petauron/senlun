<script setup lang="ts">
import type { RouteLocationNormalized } from 'vue-router'
import type { AlertRule, Incident, ManagedNode, NotificationChannel } from '@/utils/admin'
import type { AlertChannelSummary, NodeTargetScopeSummary } from '@/utils/adminEditing'
import type { AlertConfigDeleteKind, PendingUncertainConfigDeleteLock } from '@/utils/adminUncertain'
import { computed, inject, nextTick, onBeforeUnmount, ref, watch } from 'vue'
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
  buildSelectableChannels,
  cloneAlertRule,
  cloneNotificationChannel,
  createClientUuid,
  isAlertRuleDraftDirty,
  isChannelDraftDirty,
  pruneMissingChannels,
  requiresGlobalScopeConfirmation,
  scrollAndFocusHeading,
  shouldGuardGlobalScope,
  summarizeAlertChannels,
  summarizeNodeTargetScope,
  validateAlertDraftChannels,
  validateScopedNodeSelection,
} from '@/utils/adminEditing'
import {
  alertConfigDeleteUncertainLocks,
  isSameActiveSession,
  setAlertConfigDeleteUncertainLock,
} from '@/utils/adminUncertain'
import { getSharedApi } from '@/utils/api'
import { trafficSelectionDetails } from '@/utils/nodeHelpers'
import { getSessionGeneration } from '@/utils/session'

interface PendingUncertainChannelSave {
  targetId: string
  wasExisting: boolean
  name: string
  enabled: boolean
  urlChanged: boolean
  revision: number
  message: string
  resyncError: string
}

interface PendingUncertainRuleSave {
  targetId: string
  wasExisting: boolean
  name: string
  submitted: AlertRule
  revision: number
  message: string
  resyncError: string
}

interface PendingAlertRemoveTarget {
  kind: AlertConfigDeleteKind
  id: string
  name: string
}

const props = defineProps<{ channels: NotificationChannel[], rules: AlertRule[], incidents: Incident[], failures: { happened_at_unix_ms: number, subject: string }[], nodes: ManagedNode[], refresh: () => Promise<void> }>()
const emit = defineEmits<{ channelDeleted: [id: string], ruleDeleted: [id: string] }>()
const auth = useAuthStore()
const draftRegistry = inject(ADMIN_DRAFT_REGISTRY_KEY, null)

function emptyChannel(): NotificationChannel {
  return {
    id: '',
    name: '',
    kind: 'webhook',
    url: '',
    enabled: false,
  }
}

function emptyRule(): AlertRule {
  return {
    id: '',
    name: '',
    metric: 'offline',
    threshold: 0,
    duration_seconds: 120,
    cooldown_seconds: 900,
    node_ids: [],
    channel_ids: [],
    enabled: true,
  }
}

const channel = ref<NotificationChannel>(emptyChannel())
const channelBaseline = ref<NotificationChannel>(emptyChannel())
const pendingNewChannelId = ref('')
const seenPendingNewChannelId = ref('')
const removedChannelName = ref('')
const channelEditRevision = ref(0)
const pendingUncertainChannelSave = ref<PendingUncertainChannelSave | null>(null)
const channelFormHeadingRef = ref<HTMLElement | null>(null)

const rule = ref<AlertRule>(emptyRule())
const ruleBaseline = ref<AlertRule>(emptyRule())
const pendingNewRuleId = ref('')
const seenPendingNewRuleId = ref('')
const removedRuleName = ref('')
const unverifiedScopedRuleTarget = ref<{ id: string, nodeIds: string[], enabled: boolean } | null>(null)
const ruleEditRevision = ref(0)
const confirmGlobalScope = ref(false)
const pendingUncertainRuleSave = ref<PendingUncertainRuleSave | null>(null)
const ruleValidationError = ref('')
const ruleFormHeadingRef = ref<HTMLElement | null>(null)

const refreshBusy = ref(false)
const removingTarget = ref<PendingAlertRemoveTarget | null>(null)
const lastDeleteKind = ref<AlertConfigDeleteKind | null>(null)
const pendingDeleteLocks = alertConfigDeleteUncertainLocks

const channelAction = useAdminAction(props.refresh)
const ruleAction = useAdminAction(props.refresh)
const deleteAction = useAdminAction(props.refresh)

const {
  busy: channelBusy,
  error: channelError,
  resyncError: channelResyncError,
  success: channelSuccess,
  needsResync: channelNeedsResync,
} = channelAction
const {
  busy: ruleBusy,
  error: ruleError,
  resyncError: ruleResyncError,
  success: ruleSuccess,
  needsResync: ruleNeedsResync,
} = ruleAction
const {
  busy: deleteBusy,
  error: deleteError,
  resyncError: deleteResyncError,
  success: deleteSuccess,
  outcome: deleteOutcome,
  needsResync: deleteNeedsResync,
} = deleteAction

const busy = computed(() => channelBusy.value || ruleBusy.value || deleteBusy.value || refreshBusy.value)
const hasUnsavedChannel = computed(() => isChannelDraftDirty(channel.value, channelBaseline.value))
const hasUnsavedRule = computed(() => isAlertRuleDraftDirty(rule.value, ruleBaseline.value, confirmGlobalScope.value))
const hasAnyUnsavedChanges = computed(() => hasUnsavedChannel.value || hasUnsavedRule.value)
const effectiveChannelId = computed(() => channel.value.id || pendingNewChannelId.value)
const effectiveRuleId = computed(() => rule.value.id || pendingNewRuleId.value)
const liveEditingRule = computed(() => effectiveRuleId.value
  ? props.rules.find(item => item.id === effectiveRuleId.value)
  : undefined)
const editingChannelMissing = computed(() => Boolean(channel.value.id) && !props.channels.some(item => item.id === channel.value.id))
const editingRuleMissing = computed(() => Boolean(rule.value.id) && !props.rules.some(item => item.id === rule.value.id))

// Combine latest GET snapshot with any still-unverified scoped submission for the same ID so neither stale GET nor 400 retries can bypass #312.
const matchingUnverifiedRuleScope = computed(() => {
  const pending = unverifiedScopedRuleTarget.value
  if (!pending || !effectiveRuleId.value || pending.id !== effectiveRuleId.value)
    return null
  return pending
})
const persistedRuleBaseScope = computed(() => {
  if (liveEditingRule.value)
    return liveEditingRule.value
  if (rule.value.id)
    return ruleBaseline.value
  return undefined
})
const persistedNodeIds = computed(() => {
  const ids = new Set<string>()
  if (persistedRuleBaseScope.value) {
    for (const id of persistedRuleBaseScope.value.node_ids)
      ids.add(id)
  }
  if (matchingUnverifiedRuleScope.value) {
    for (const id of matchingUnverifiedRuleScope.value.nodeIds)
      ids.add(id)
  }
  return Array.from(ids)
})
const guardGlobalScope = computed(() => {
  if (persistedRuleBaseScope.value && shouldGuardGlobalScope(persistedRuleBaseScope.value))
    return true
  if (matchingUnverifiedRuleScope.value)
    return true
  return false
})
const persistedRuleScopeSignature = computed(() => {
  const basePart = persistedRuleBaseScope.value
    ? `${persistedRuleBaseScope.value.id}|${persistedRuleBaseScope.value.enabled ? '1' : '0'}|${[...persistedRuleBaseScope.value.node_ids].sort().join(',')}`
    : ''
  const unverifiedPart = matchingUnverifiedRuleScope.value
    ? `${matchingUnverifiedRuleScope.value.id}|${matchingUnverifiedRuleScope.value.enabled ? '1' : '0'}|${[...matchingUnverifiedRuleScope.value.nodeIds].sort().join(',')}`
    : ''
  return `${basePart}::${unverifiedPart}`
})

const dirtyDraftSummary = computed(() => {
  if (hasUnsavedChannel.value && hasUnsavedRule.value)
    return '通知渠道与告警规则'
  if (hasUnsavedChannel.value)
    return '通知渠道'
  if (hasUnsavedRule.value)
    return '告警规则'
  return ''
})

const assignableNodes = computed(() => buildAssignableNodes(props.nodes, rule.value.node_ids, persistedNodeIds.value))
const selectableChannels = computed(() => buildSelectableChannels(props.channels, rule.value.channel_ids))
const hasMissingChannels = computed(() => selectableChannels.value.some(item => item.missing))
const needsGlobalConfirmation = computed(() => requiresGlobalScopeConfirmation(
  guardGlobalScope.value,
  rule.value.node_ids,
  rule.value.enabled,
))

const channelAlert = computed(() => {
  const pending = pendingUncertainChannelSave.value
  if (pending) {
    return pending.resyncError
      ? `${pending.message}（重新同步失败：${pending.resyncError}）`
      : pending.message
  }
  return channelError.value
})
const showChannelResync = computed(() => channelNeedsResync.value || Boolean(pendingUncertainChannelSave.value))

const ruleAlert = computed(() => {
  const pending = pendingUncertainRuleSave.value
  if (pending) {
    return pending.resyncError
      ? `${pending.message}（重新同步失败：${pending.resyncError}）`
      : pending.message
  }
  return ruleError.value
})
const showRuleResync = computed(() => ruleNeedsResync.value || Boolean(pendingUncertainRuleSave.value))

const channelDeleteLocks = computed(() => Object.values(pendingDeleteLocks.value).filter(item => item.kind === 'channels'))
const ruleDeleteLocks = computed(() => Object.values(pendingDeleteLocks.value).filter(item => item.kind === 'alert-rules'))

const activeRemovingUncertain = computed(() => {
  const target = removingTarget.value
  if (!target)
    return null
  return pendingDeleteLocks.value[`${target.kind}:${target.id}`] ?? null
})

const removingTargetInvalidReason = computed(() => {
  const target = removingTarget.value
  if (!target)
    return ''
  if (target.kind === 'channels' && !props.channels.some(item => item.id === target.id))
    return `通知渠道「${target.name}」在最新同步中已不存在，本次确认已失效，无法执行。`
  if (target.kind === 'alert-rules' && !props.rules.some(item => item.id === target.id))
    return `告警规则「${target.name}」在最新同步中已不存在，本次确认已失效，无法执行。`
  return ''
})

const removingDraftWarning = computed(() => {
  const target = removingTarget.value
  if (!target || removingTargetInvalidReason.value)
    return ''
  if (target.kind === 'channels' && effectiveChannelId.value === target.id && hasUnsavedChannel.value)
    return '当前通知渠道表单中针对此渠道的未保存修改也会一并丢弃（不影响告警规则草稿）。'
  if (target.kind === 'alert-rules' && effectiveRuleId.value === target.id && hasUnsavedRule.value)
    return '当前告警规则表单中针对此规则的未保存修改也会一并丢弃（不影响通知渠道草稿）。'
  return ''
})

const removingDialogTitle = computed(() => removingTarget.value?.kind === 'channels'
  ? '删除通知渠道'
  : '删除告警规则')

const removingConfirmLabel = computed(() => {
  if (deleteBusy.value)
    return '正在删除…'
  return removingTarget.value?.kind === 'channels'
    ? '确认删除通知渠道'
    : '确认删除告警规则'
})

const showChannelDeleteRefreshAlert = computed(() => removingTarget.value === null
  && lastDeleteKind.value === 'channels'
  && deleteOutcome.value === 'refresh_failed'
  && Boolean(deleteError.value))

const showRuleDeleteRefreshAlert = computed(() => removingTarget.value === null
  && lastDeleteKind.value === 'alert-rules'
  && deleteOutcome.value === 'refresh_failed'
  && Boolean(deleteError.value))

const metricLabels = {
  offline: '节点离线',
  cpu: 'CPU 使用率',
  memory: '内存使用率',
  disk: '磁盘使用率',
  traffic: '所选来源流量额度使用率',
  expiry: '到期提醒',
}
const metricOptions = Object.entries(metricLabels).map(([value, label]) => ({ value, label }))
const metricLabel = (metric: keyof typeof metricLabels) => metricLabels[metric]
const thresholdLabel = computed(() => rule.value.metric === 'expiry'
  ? '提前提醒（天）'
  : '阈值（%）')
const date = (value: number) => new Date(value).toLocaleString()
const nodeName = (id: string) => props.nodes.find(node => node.id === id)?.name ?? id
const incidentLabels: Record<string, string> = {
  pending: '等待持续条件',
  firing: '告警中',
  resolved: '已恢复',
  superseded: '已结束（不代表恢复）',
}
const incidentStatus = (status: string) => incidentLabels[status] ?? status

function formatPendingAlert(message: string, resyncFailure: string): string {
  return resyncFailure
    ? `${message}（重新同步失败：${resyncFailure}）`
    : message
}

function ruleScopeSummary(item: AlertRule): NodeTargetScopeSummary {
  return summarizeNodeTargetScope(item.node_ids, props.nodes)
}

function ruleChannelSummary(item: AlertRule): AlertChannelSummary {
  return summarizeAlertChannels(item.channel_ids, props.channels)
}

function focusChannelFormHeading(): void {
  void nextTick(() => scrollAndFocusHeading(channelFormHeadingRef.value))
}

function focusRuleFormHeading(): void {
  void nextTick(() => scrollAndFocusHeading(ruleFormHeadingRef.value))
}

function applyResetChannel(clearFeedback = true): void {
  channel.value = emptyChannel()
  channelBaseline.value = emptyChannel()
  pendingNewChannelId.value = ''
  seenPendingNewChannelId.value = ''
  removedChannelName.value = ''
  pendingUncertainChannelSave.value = null
  if (clearFeedback)
    channelAction.clearFeedback()
}

function applyEditChannel(item: NotificationChannel, clearFeedback = true): void {
  channel.value = cloneNotificationChannel(item)
  channelBaseline.value = cloneNotificationChannel(item)
  pendingNewChannelId.value = ''
  seenPendingNewChannelId.value = ''
  removedChannelName.value = ''
  pendingUncertainChannelSave.value = null
  if (clearFeedback)
    channelAction.clearFeedback()
}

function markPendingNewChannelRemoved(removedId: string, displayName: string): void {
  if (!channel.value.id && pendingNewChannelId.value === removedId) {
    channel.value.id = removedId
    removedChannelName.value = displayName
    pendingNewChannelId.value = ''
    seenPendingNewChannelId.value = ''
    pendingUncertainChannelSave.value = null
  }
}

function applyResetRule(clearFeedback = true): void {
  rule.value = emptyRule()
  ruleBaseline.value = emptyRule()
  pendingNewRuleId.value = ''
  seenPendingNewRuleId.value = ''
  removedRuleName.value = ''
  unverifiedScopedRuleTarget.value = null
  confirmGlobalScope.value = false
  pendingUncertainRuleSave.value = null
  ruleValidationError.value = ''
  if (clearFeedback)
    ruleAction.clearFeedback()
}

function applyEditRule(item: AlertRule, clearFeedback = true): void {
  rule.value = cloneAlertRule(item)
  ruleBaseline.value = cloneAlertRule(item)
  pendingNewRuleId.value = ''
  seenPendingNewRuleId.value = ''
  removedRuleName.value = ''
  unverifiedScopedRuleTarget.value = null
  confirmGlobalScope.value = false
  pendingUncertainRuleSave.value = null
  ruleValidationError.value = ''
  if (clearFeedback)
    ruleAction.clearFeedback()
}

function markPendingNewRuleRemoved(removedId: string, displayName: string): void {
  if (!rule.value.id && pendingNewRuleId.value === removedId) {
    rule.value.id = removedId
    removedRuleName.value = displayName
    pendingNewRuleId.value = ''
    seenPendingNewRuleId.value = ''
    unverifiedScopedRuleTarget.value = null
    pendingUncertainRuleSave.value = null
  }
}

watch(() => rule.value.node_ids.length, (count) => {
  if (count > 0)
    confirmGlobalScope.value = false
})

watch(persistedRuleScopeSignature, (next, prev) => {
  if (next !== prev)
    confirmGlobalScope.value = false
})

watch(channel, () => {
  if (!refreshBusy.value)
    channelEditRevision.value++
}, { deep: true, flush: 'sync' })

watch(
  [
    rule,
    confirmGlobalScope,
  ],
  () => {
    if (!refreshBusy.value)
      ruleEditRevision.value++
  },
  { deep: true, flush: 'sync' },
)

watch(
  () => props.channels,
  (channels) => {
    if (!channel.value.id) {
      if (pendingNewChannelId.value) {
        const created = channels.find(item => item.id === pendingNewChannelId.value)
        if (created) {
          seenPendingNewChannelId.value = pendingNewChannelId.value
        }
        else if (seenPendingNewChannelId.value === pendingNewChannelId.value) {
          if (!hasUnsavedChannel.value)
            applyResetChannel(false)
          else
            markPendingNewChannelRemoved(pendingNewChannelId.value, channel.value.name || pendingNewChannelId.value)
        }
      }
      return
    }
    const live = channels.find(item => item.id === channel.value.id)
    if (!live) {
      if (!hasUnsavedChannel.value)
        applyResetChannel(false)
      return
    }
    if (!hasUnsavedChannel.value) {
      applyEditChannel(live, false)
      return
    }
    if (!isChannelDraftDirty(channel.value, live))
      channelBaseline.value = cloneNotificationChannel(live)
  },
)

watch(
  () => props.rules,
  (rules) => {
    if (!rule.value.id) {
      if (pendingNewRuleId.value) {
        const created = rules.find(item => item.id === pendingNewRuleId.value)
        if (created) {
          seenPendingNewRuleId.value = pendingNewRuleId.value
        }
        else if (seenPendingNewRuleId.value === pendingNewRuleId.value) {
          if (!hasUnsavedRule.value)
            applyResetRule(false)
          else
            markPendingNewRuleRemoved(pendingNewRuleId.value, rule.value.name || pendingNewRuleId.value)
        }
      }
      return
    }
    const live = rules.find(item => item.id === rule.value.id)
    if (!live) {
      if (!hasUnsavedRule.value)
        applyResetRule(false)
      return
    }
    if (!hasUnsavedRule.value) {
      applyEditRule(live, false)
      return
    }
    if (!isAlertRuleDraftDirty(rule.value, live, false))
      ruleBaseline.value = cloneAlertRule(live)
  },
)

function isStayingOnAlertsSection(to: RouteLocationNormalized): boolean {
  return to.name === 'admin' && to.query.section === 'alerts'
}

const draftGuard = useAdminDraftGuard({
  isLoggedIn: () => auth.loggedIn,
  isDirty: () => hasAnyUnsavedChanges.value,
  isBusy: () => busy.value,
  isStayingInSameContext: isStayingOnAlertsSection,
})

const {
  pending: draftGuardPending,
  isOpen: draftGuardOpen,
  cancel: cancelDraftGuard,
  confirm: confirmDraftGuard,
  restoreFocus: restoreDraftGuardFocus,
} = draftGuard

const draftGuardDescription = computed(() => {
  const action = draftGuardPending.value
  if (!action)
    return ''
  if (action.kind === 'switch') {
    if (action.targetId.startsWith('channel:'))
      return `切换到 ${action.targetLabel} 会丢弃当前通知渠道表单未保存的修改，不会影响下方告警规则草稿。`
    return `切换到 ${action.targetLabel} 会丢弃当前告警规则表单未保存的修改，不会影响上方通知渠道草稿。`
  }
  const summary = dirtyDraftSummary.value || '当前表单'
  if (action.kind === 'refresh')
    return `刷新管理数据会丢弃当前未保存的${summary}修改，不会保存任何更改。`
  return `离开通知与告警栏目会丢弃当前未保存的${summary}修改，不会保存任何更改。`
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
  section: 'alerts',
  isDirty: () => hasAnyUnsavedChanges.value,
  isBusy: () => busy.value,
  requestRefresh: triggerEl => draftGuard.requestRefresh(async () => {
    const confirmedChannelId = channel.value.id
    const confirmedChannelRevision = channelEditRevision.value
    const confirmedRuleId = rule.value.id
    const confirmedRuleRevision = ruleEditRevision.value
    const hadDirtyChannel = hasUnsavedChannel.value
    const hadDirtyRule = hasUnsavedRule.value
    refreshBusy.value = true
    channelAction.clearFeedback()
    ruleAction.clearFeedback()
    try {
      await props.refresh()
      if (channel.value.id === confirmedChannelId && channelEditRevision.value === confirmedChannelRevision) {
        pendingUncertainChannelSave.value = null
        const liveChannel = confirmedChannelId
          ? props.channels.find(item => item.id === confirmedChannelId)
          : undefined
        if (liveChannel)
          applyEditChannel(liveChannel, false)
        else
          applyResetChannel(false)
      }
      if (rule.value.id === confirmedRuleId && ruleEditRevision.value === confirmedRuleRevision) {
        pendingUncertainRuleSave.value = null
        const liveRule = confirmedRuleId
          ? props.rules.find(item => item.id === confirmedRuleId)
          : undefined
        if (liveRule)
          applyEditRule(liveRule, false)
        else
          applyResetRule(false)
      }
    }
    catch (cause) {
      const detail = cause instanceof Error
        ? cause.message
        : String(cause)
      if (hadDirtyChannel)
        channelError.value = detail
      if (hadDirtyRule || !hadDirtyChannel)
        ruleError.value = detail
    }
    finally {
      refreshBusy.value = false
    }
  }, triggerEl),
})

onBeforeUnmount(() => {
  unregisterDraft?.()
})

function editChannel(item: NotificationChannel, event?: MouseEvent): void {
  if (busy.value)
    return
  const triggerEl = event?.currentTarget instanceof HTMLElement
    ? event.currentTarget
    : null
  if (triggerEl)
    triggerEl.focus()
  if (channel.value.id === item.id) {
    focusChannelFormHeading()
    return
  }
  if (!hasUnsavedChannel.value) {
    applyEditChannel(item)
    focusChannelFormHeading()
    return
  }
  draftGuard.requestSwitch(
    `channel:${item.id}`,
    `通知渠道「${item.name}」`,
    () => applyEditChannel(item),
    triggerEl,
    focusChannelFormHeading,
  )
}

function cancelChannelEdit(event?: MouseEvent): void {
  if (busy.value)
    return
  const triggerEl = event?.currentTarget instanceof HTMLElement
    ? event.currentTarget
    : null
  if (triggerEl)
    triggerEl.focus()
  if (!hasUnsavedChannel.value) {
    applyResetChannel(true)
    return
  }
  draftGuard.requestSwitch(
    'channel:__new__',
    '新增通知渠道',
    () => applyResetChannel(true),
    triggerEl,
  )
}

function editRule(item: AlertRule, event?: MouseEvent): void {
  if (busy.value)
    return
  const triggerEl = event?.currentTarget instanceof HTMLElement
    ? event.currentTarget
    : null
  if (triggerEl)
    triggerEl.focus()
  if (rule.value.id === item.id) {
    focusRuleFormHeading()
    return
  }
  if (!hasUnsavedRule.value) {
    applyEditRule(item)
    focusRuleFormHeading()
    return
  }
  draftGuard.requestSwitch(
    `rule:${item.id}`,
    `告警规则「${item.name}」`,
    () => applyEditRule(item),
    triggerEl,
    focusRuleFormHeading,
  )
}

function cancelRuleEdit(event?: MouseEvent): void {
  if (busy.value)
    return
  const triggerEl = event?.currentTarget instanceof HTMLElement
    ? event.currentTarget
    : null
  if (triggerEl)
    triggerEl.focus()
  if (!hasUnsavedRule.value) {
    applyResetRule(true)
    return
  }
  draftGuard.requestSwitch(
    'rule:__new__',
    '新增告警规则',
    () => applyResetRule(true),
    triggerEl,
  )
}

function clearMissingChannels(): void {
  rule.value.channel_ids = pruneMissingChannels(rule.value.channel_ids, props.channels)
  ruleValidationError.value = ''
  ruleError.value = ''
}

function openRemove(item: PendingAlertRemoveTarget): void {
  if (busy.value)
    return
  if (deleteOutcome.value === 'failed' || deleteOutcome.value === 'saved')
    deleteAction.clearFeedback()
  removingTarget.value = { ...item }
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

async function saveChannel(): Promise<void> {
  if (editingChannelMissing.value)
    return
  const wasExisting = Boolean(channel.value.id)
  if (!wasExisting && !pendingNewChannelId.value)
    pendingNewChannelId.value = createClientUuid()
  const targetId = wasExisting
    ? channel.value.id
    : pendingNewChannelId.value
  const payload = cloneNotificationChannel({
    ...channel.value,
    id: targetId,
  })
  const submittedRevision = channelEditRevision.value
  const urlChanged = payload.url !== channelBaseline.value.url
  pendingUncertainChannelSave.value = null
  const result = await channelAction.run(async () => {
    await getSharedApi().post('admin/channels', payload)
  }, {
    message: '已保存通知渠道',
    onCommitted: () => {
      applyResetChannel(false)
    },
  })
  if (result === 'uncertain') {
    // Store minimal metadata only (never copy webhook URL / tokens into secondary state, shared locks, or alerts).
    pendingUncertainChannelSave.value = {
      targetId,
      wasExisting,
      name: payload.name,
      enabled: Boolean(payload.enabled),
      urlChanged,
      revision: submittedRevision,
      message: channelError.value,
      resyncError: '',
    }
  }
}

async function resyncChannel(): Promise<void> {
  const pending = pendingUncertainChannelSave.value
  if (!pending) {
    await channelAction.resync('已重新同步最新状态')
    return
  }
  pending.resyncError = ''
  const ok = await channelAction.resync({
    verify: () => {
      pendingUncertainChannelSave.value = null
      const fresh = props.channels.find(item => item.id === pending.targetId)
      if (!fresh) {
        return {
          resolved: true,
          message: pending.wasExisting
            ? `已核实：通知渠道「${pending.name}」已不存在`
            : `已同步最新状态：列表中尚未出现本次提交的新通知渠道「${pending.name}」，已保留当前输入与同一标识，请核对后再决定是否重新保存。`,
        }
      }
      const sameDraftContext = pending.wasExisting
        ? channel.value.id === pending.targetId
        : !channel.value.id && pendingNewChannelId.value === pending.targetId
      const editedAfterSubmit = sameDraftContext && channelEditRevision.value !== pending.revision
      const exactUneditedMatch = sameDraftContext
        && !editedAfterSubmit
        && !isChannelDraftDirty({
          ...channel.value,
          id: fresh.id,
        }, fresh)
      if (exactUneditedMatch) {
        if (pending.wasExisting)
          applyEditChannel(fresh, false)
        else
          applyResetChannel(false)
        return {
          resolved: true,
          message: pending.wasExisting
            ? `已核实：通知渠道「${pending.name}」的修改已在服务端生效`
            : `已核实：新通知渠道「${pending.name}」已在服务端创建`,
        }
      }
      const metaMatches = fresh.name === pending.name && Boolean(fresh.enabled) === pending.enabled
      if (sameDraftContext && !pending.wasExisting) {
        channel.value.id = fresh.id
        pendingNewChannelId.value = ''
        seenPendingNewChannelId.value = ''
        channelBaseline.value = cloneNotificationChannel(fresh)
      }
      else if (sameDraftContext && metaMatches && !pending.urlChanged) {
        channelBaseline.value = cloneNotificationChannel(fresh)
      }
      if (metaMatches && !pending.urlChanged && editedAfterSubmit) {
        return {
          resolved: true,
          message: pending.wasExisting
            ? `已核实：上次提交的通知渠道「${pending.name}」修改已在服务端生效；您之后的新修改已保留，尚未保存。`
            : `已核实：新通知渠道「${pending.name}」已创建；您之后的新修改已保留，尚未保存。`,
        }
      }
      return {
        resolved: true,
        message: `已同步最新状态：服务端已存在该标识对应的通知渠道「${pending.name}」，但当前表单包含未核实或更新后的修改（含 URL 变更），已保留当前输入，请核对后再决定是否重新保存。`,
      }
    },
  })
  if (!ok && pendingUncertainChannelSave.value)
    pendingUncertainChannelSave.value.resyncError = channelResyncError.value
}

async function saveRule(): Promise<void> {
  if (editingRuleMissing.value)
    return
  ruleValidationError.value = ''
  const scopeError = validateScopedNodeSelection({
    guardGlobalScope: guardGlobalScope.value,
    selectedIds: rule.value.node_ids,
    enabled: rule.value.enabled,
    confirmGlobalScope: confirmGlobalScope.value,
    nodes: props.nodes,
  })
  if (scopeError) {
    ruleValidationError.value = scopeError
    ruleSuccess.value = ''
    return
  }
  const channelValidationError = validateAlertDraftChannels(rule.value.channel_ids, props.channels)
  if (channelValidationError) {
    ruleValidationError.value = channelValidationError
    ruleSuccess.value = ''
    return
  }
  const wasExisting = Boolean(rule.value.id)
  if (!wasExisting && !pendingNewRuleId.value)
    pendingNewRuleId.value = createClientUuid()
  const targetId = wasExisting
    ? rule.value.id
    : pendingNewRuleId.value
  const submitted = cloneAlertRule({
    ...rule.value,
    id: targetId,
  })
  const submittedRevision = ruleEditRevision.value
  pendingUncertainRuleSave.value = null
  const result = await ruleAction.run(async () => {
    await getSharedApi().post('admin/alert-rules', submitted)
  }, {
    message: '已保存告警规则',
    onCommitted: () => {
      applyResetRule(false)
    },
  })
  if (result === 'uncertain') {
    if (shouldGuardGlobalScope(submitted)) {
      unverifiedScopedRuleTarget.value = {
        id: targetId,
        nodeIds: [...submitted.node_ids],
        enabled: Boolean(submitted.enabled),
      }
    }
    pendingUncertainRuleSave.value = {
      targetId,
      wasExisting,
      name: submitted.name,
      submitted,
      revision: submittedRevision,
      message: ruleError.value,
      resyncError: '',
    }
  }
}

async function resyncRule(): Promise<void> {
  ruleValidationError.value = ''
  const pending = pendingUncertainRuleSave.value
  if (!pending) {
    await ruleAction.resync('已重新同步最新状态')
    return
  }
  pending.resyncError = ''
  const ok = await ruleAction.resync({
    verify: () => {
      pendingUncertainRuleSave.value = null
      const fresh = props.rules.find(item => item.id === pending.targetId)
      if (fresh && shouldGuardGlobalScope(fresh) && unverifiedScopedRuleTarget.value?.id === pending.targetId)
        unverifiedScopedRuleTarget.value = null
      if (!fresh) {
        return {
          resolved: true,
          message: pending.wasExisting
            ? `已核实：告警规则「${pending.name}」已不存在`
            : `已同步最新状态：列表中尚未出现新告警规则「${pending.name}」，已保留当前输入与同一标识，请核对后再决定是否重新保存。`,
        }
      }
      const applied = !isAlertRuleDraftDirty(pending.submitted, fresh, false)
      const sameDraftContext = pending.wasExisting
        ? rule.value.id === pending.targetId
        : !rule.value.id && pendingNewRuleId.value === pending.targetId
      const editedAfterSubmit = sameDraftContext && ruleEditRevision.value !== pending.revision
      if (applied) {
        if (sameDraftContext) {
          if (editedAfterSubmit) {
            if (!pending.wasExisting) {
              rule.value.id = fresh.id
              pendingNewRuleId.value = ''
              seenPendingNewRuleId.value = ''
            }
            ruleBaseline.value = cloneAlertRule(fresh)
          }
          else if (pending.wasExisting) {
            applyEditRule(fresh, false)
          }
          else {
            applyResetRule(false)
          }
        }
        if (pending.wasExisting) {
          return {
            resolved: true,
            message: editedAfterSubmit
              ? `已核实：上次提交的告警规则「${pending.name}」修改已在服务端生效；您之后的新修改已保留，尚未保存。`
              : `已核实：告警规则「${pending.name}」的修改已在服务端生效`,
          }
        }
        return {
          resolved: true,
          message: editedAfterSubmit
            ? `已核实：新告警规则「${pending.name}」已创建；您之后的新修改已保留，尚未保存。`
            : `已核实：新告警规则「${pending.name}」已在服务端创建`,
        }
      }
      if (sameDraftContext && !pending.wasExisting) {
        rule.value.id = fresh.id
        pendingNewRuleId.value = ''
        seenPendingNewRuleId.value = ''
        ruleBaseline.value = cloneAlertRule(fresh)
      }
      return {
        resolved: true,
        message: `已同步最新状态：服务端告警规则「${pending.name}」与刚才提交不一致，已保留当前输入，请核对后再决定是否重新保存。`,
      }
    },
  })
  if (!ok && pendingUncertainRuleSave.value)
    pendingUncertainRuleSave.value.resyncError = ruleResyncError.value
}

async function remove(): Promise<void> {
  if (!removingTarget.value || activeRemovingUncertain.value || removingTargetInvalidReason.value)
    return
  const sessionGen = getSessionGeneration()
  const item = { ...removingTarget.value }
  const key = `${item.kind}:${item.id}`
  const label = item.kind === 'channels'
    ? '通知渠道'
    : '告警规则'
  lastDeleteKind.value = item.kind
  let committedDelete = false
  const result = await deleteAction.run(async () => {
    await getSharedApi().post(`admin/${item.kind}/${encodeURIComponent(item.id)}/delete`)
  }, {
    message: `已删除${label}「${item.name}」`,
    riskyWrite: true,
    uncertainMessage: detail => `删除${label}「${item.name}」结果未确认（${detail}）。请先重新同步核对当前列表，勿直接重复提交。`,
    onCommitted: () => {
      if (!isSameActiveSession(sessionGen, auth.loggedIn))
        return
      removingTarget.value = null
      unlockUncertainDelete(key)
      committedDelete = true
    },
  })
  if (committedDelete && isSameActiveSession(sessionGen, auth.loggedIn)) {
    if (item.kind === 'channels') {
      if (effectiveChannelId.value === item.id)
        applyResetChannel(false)
      const ruleWasClean = !hasUnsavedRule.value
      rule.value.channel_ids = rule.value.channel_ids.filter(id => id !== item.id)
      if (ruleWasClean)
        ruleBaseline.value.channel_ids = ruleBaseline.value.channel_ids.filter(id => id !== item.id)
      emit('channelDeleted', item.id)
    }
    else {
      if (effectiveRuleId.value === item.id)
        applyResetRule(false)
      emit('ruleDeleted', item.id)
    }
    return
  }
  if (result === 'uncertain') {
    setAlertConfigDeleteUncertainLock(sessionGen, auth.loggedIn, {
      key,
      kind: item.kind,
      id: item.id,
      name: item.name,
      message: deleteError.value,
      resyncError: '',
    })
  }
}

async function resyncDelete(entry?: PendingUncertainConfigDeleteLock<AlertConfigDeleteKind>): Promise<void> {
  if (!entry) {
    await deleteAction.resync('已重新同步最新状态')
    return
  }
  const sessionGen = getSessionGeneration()
  const targetWasDirtyBeforeResync = entry.kind === 'channels'
    ? effectiveChannelId.value === entry.id && hasUnsavedChannel.value
    : effectiveRuleId.value === entry.id && hasUnsavedRule.value
  entry.resyncError = ''
  const label = entry.kind === 'channels'
    ? '通知渠道'
    : '告警规则'
  lastDeleteKind.value = entry.kind
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
      const exists = entry.kind === 'channels'
        ? props.channels.some(item => item.id === entry.id)
        : props.rules.some(item => item.id === entry.id)
      if (!exists) {
        if (removingTarget.value?.kind === entry.kind && removingTarget.value.id === entry.id)
          removingTarget.value = null
        verifiedDeleted = true
        return {
          resolved: true,
          message: `已核实：${label}「${entry.name}」已删除`,
        }
      }
      return {
        resolved: true,
        message: `已核对最新状态：${label}「${entry.name}」仍存在，可确认后再决定是否重试删除。`,
      }
    },
  })
  if (verifiedDeleted && isSameActiveSession(sessionGen, auth.loggedIn)) {
    // Read-only resync must preserve dirty user edits (surfaced via editing*Missing) and block old UUID resurrection.
    if (entry.kind === 'channels') {
      if (effectiveChannelId.value === entry.id) {
        if (!targetWasDirtyBeforeResync && !hasUnsavedChannel.value)
          applyResetChannel(false)
        else
          markPendingNewChannelRemoved(entry.id, entry.name)
      }
    }
    else if (effectiveRuleId.value === entry.id) {
      if (!targetWasDirtyBeforeResync && !hasUnsavedRule.value)
        applyResetRule(false)
      else
        markPendingNewRuleRemoved(entry.id, entry.name)
    }
    return
  }
  if (!ok && deleteResyncError.value && isSameActiveSession(sessionGen, auth.loggedIn) && pendingDeleteLocks.value[entry.key])
    pendingDeleteLocks.value[entry.key]!.resyncError = deleteResyncError.value
}
</script>

<template>
  <div class="space-y-4">
    <CardX title="Webhook 通知渠道" content-class="space-y-4">
      <p class="text-sm text-muted-foreground">
        渠道默认关闭。显式启用后向配置的 HTTPS 地址发送节点名称与 ID、规则、状态、时间及简短原因；不发送 IP、凭据或原始指标快照。请妥善保管 URL 中的令牌。
      </p>
      <div v-if="showChannelDeleteRefreshAlert" class="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/40 bg-card p-3">
        <p role="alert" class="text-sm text-destructive">
          {{ deleteError }}
        </p>
        <Button v-if="deleteNeedsResync" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncDelete()">
          重新同步
        </Button>
      </div>
      <div v-if="removingTarget === null && channelDeleteLocks.length" class="space-y-2">
        <div v-for="entry in channelDeleteLocks" :key="entry.key" class="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/40 p-3">
          <p role="alert" class="text-sm text-destructive">
            {{ formatPendingAlert(entry.message, entry.resyncError) }}
          </p>
          <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncDelete(entry)">
            重新同步
          </Button>
        </div>
      </div>
      <p v-if="removingTarget === null && lastDeleteKind === 'channels' && deleteSuccess" role="status" class="text-sm text-success">
        {{ deleteSuccess }}
      </p>
      <ul v-if="channels.length" aria-label="通知渠道列表" class="space-y-2">
        <li
          v-for="item in channels"
          :key="item.id"
          :data-editing="channel.id === item.id ? 'true' : undefined"
          class="flex flex-wrap items-center justify-between gap-2 rounded-lg border p-3 transition-colors"
          :class="channel.id === item.id ? 'border-primary bg-primary/10' : 'border-border/80'"
        >
          <div class="flex min-w-0 flex-wrap items-center gap-1.5">
            <p class="font-medium break-words">
              {{ item.name }}
            </p>
            <Badge :variant="item.enabled ? 'secondary' : 'outline'">
              {{ item.enabled ? '启用' : '停用' }}
            </Badge>
            <Badge v-if="channel.id === item.id" variant="default">
              正在编辑
            </Badge>
            <Badge v-if="channel.id === item.id && hasUnsavedChannel" variant="outline">
              未保存
            </Badge>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="editChannel(item, $event)">
              编辑
            </Button>
            <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="openRemove({ kind: 'channels', id: item.id, name: item.name })">
              删除
            </Button>
          </div>
        </li>
      </ul>
      <form class="space-y-4 border-t pt-4" :aria-busy="busy" @submit.prevent="saveChannel">
        <div class="flex flex-wrap items-center gap-2">
          <h3 ref="channelFormHeadingRef" tabindex="-1" class="font-medium outline-none">
            {{ channel.id ? '编辑渠道' : '新增渠道' }}
          </h3>
          <Badge v-if="channel.id && (channelBaseline.name || removedChannelName)" variant="secondary">
            {{ channelBaseline.name || removedChannelName }}
          </Badge>
          <Badge v-if="hasUnsavedChannel" variant="outline">
            未保存
          </Badge>
        </div>
        <p v-if="editingChannelMissing" role="alert" class="rounded-md border border-destructive/40 p-3 text-sm text-destructive">
          当前正在编辑的通知渠道「{{ channelBaseline.name || removedChannelName || channel.id }}」在最新同步中已不存在，无法继续保存。当前未保存输入仍保留在表单中，请点击“取消编辑”返回新增态。
        </p>
        <p v-else-if="hasUnsavedChannel" role="status" class="text-sm text-muted-foreground">
          通知渠道表单有未保存修改；仅在切换通知渠道、离开栏目或主动刷新时提示，不会影响下方告警规则草稿。
        </p>
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-2">
          <label class="grid gap-2 text-sm">渠道名称<Input v-model="channel.name" class="h-11" required maxlength="128" /></label>
          <label class="grid gap-2 text-sm">Webhook URL<Input v-model="channel.url" class="h-11" type="url" pattern="https://.*" autocomplete="off" required maxlength="2048" placeholder="https://" /></label>
          <label class="flex min-h-11 items-center gap-2 text-sm sm:col-span-2"><input v-model="channel.enabled" type="checkbox" class="size-4 accent-primary">启用渠道并允许发送上述告警信息</label>
        </fieldset>
        <div class="space-y-2">
          <div class="flex flex-wrap items-center gap-2">
            <Button type="submit" class="min-h-11" :disabled="busy || editingChannelMissing">
              {{ channelBusy ? '正在保存…' : '保存渠道' }}
            </Button>
            <Button v-if="showChannelResync" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncChannel">
              重新同步
            </Button>
            <Button v-if="channel.id" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="cancelChannelEdit($event)">
              取消编辑
            </Button>
          </div>
          <p v-if="channelAlert" role="alert" class="text-sm text-destructive">
            {{ channelAlert }}
          </p>
          <p v-if="channelSuccess" role="status" class="text-sm text-success">
            {{ channelSuccess }}
          </p>
        </div>
      </form>
    </CardX>

    <CardX title="告警规则" content-class="space-y-4">
      <p class="text-sm text-muted-foreground">
        流量告警使用每个节点已保存的流量来源，与卡片和列表一致。服务商数据过期、失败或额度缺失时暂停判定，不切回本地，也不将缺数据当作恢复。
      </p>
      <div v-if="showRuleDeleteRefreshAlert" class="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/40 bg-card p-3">
        <p role="alert" class="text-sm text-destructive">
          {{ deleteError }}
        </p>
        <Button v-if="deleteNeedsResync" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncDelete()">
          重新同步
        </Button>
      </div>
      <div v-if="removingTarget === null && ruleDeleteLocks.length" class="space-y-2">
        <div v-for="entry in ruleDeleteLocks" :key="entry.key" class="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/40 p-3">
          <p role="alert" class="text-sm text-destructive">
            {{ formatPendingAlert(entry.message, entry.resyncError) }}
          </p>
          <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncDelete(entry)">
            重新同步
          </Button>
        </div>
      </div>
      <p v-if="removingTarget === null && lastDeleteKind === 'alert-rules' && deleteSuccess" role="status" class="text-sm text-success">
        {{ deleteSuccess }}
      </p>
      <p v-if="!rules.length" class="text-sm text-muted-foreground">
        尚无告警规则。选择告警条件即可启用站内告警，通知渠道可按需配置。
      </p>
      <ul v-else aria-label="告警规则列表" class="space-y-2">
        <li
          v-for="item in rules"
          :key="item.id"
          :data-editing="rule.id === item.id ? 'true' : undefined"
          class="flex flex-wrap items-center justify-between gap-2 rounded-lg border p-3 transition-colors"
          :class="rule.id === item.id ? 'border-primary bg-primary/10' : 'border-border/80'"
        >
          <div class="min-w-0 flex-1 space-y-1">
            <div class="flex flex-wrap items-center gap-1.5">
              <p class="font-medium break-words">
                {{ item.name }}
              </p>
              <Badge :variant="item.enabled ? 'secondary' : 'outline'">
                {{ item.enabled ? '启用' : '停用' }}
              </Badge>
              <Badge v-if="rule.id === item.id" variant="default">
                正在编辑
              </Badge>
              <Badge v-if="rule.id === item.id && hasUnsavedRule" variant="outline">
                未保存
              </Badge>
            </div>
            <p class="text-sm text-muted-foreground">
              {{ metricLabel(item.metric) }} · 持续 {{ item.duration_seconds }} 秒 · 冷却 {{ item.cooldown_seconds }} 秒
            </p>
            <div class="flex flex-wrap items-center gap-1.5 text-xs">
              <Badge variant="outline">
                {{ ruleScopeSummary(item).label }}
              </Badge>
              <Badge v-if="ruleScopeSummary(item).warningLabel" variant="destructive">
                {{ ruleScopeSummary(item).warningLabel }}
              </Badge>
              <Badge variant="outline">
                {{ ruleChannelSummary(item).label }}
              </Badge>
              <Badge v-if="ruleChannelSummary(item).warningLabel" :variant="ruleChannelSummary(item).missingCount > 0 ? 'destructive' : 'secondary'">
                {{ ruleChannelSummary(item).warningLabel }}
              </Badge>
            </div>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="editRule(item, $event)">
              编辑
            </Button>
            <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="openRemove({ kind: 'alert-rules', id: item.id, name: item.name })">
              删除
            </Button>
          </div>
        </li>
      </ul>
      <form class="space-y-4 border-t pt-4" :aria-busy="busy" @submit.prevent="saveRule">
        <div class="flex flex-wrap items-center gap-2">
          <h3 ref="ruleFormHeadingRef" tabindex="-1" class="font-medium outline-none">
            {{ rule.id ? '编辑规则' : '新增规则' }}
          </h3>
          <Badge v-if="rule.id && (ruleBaseline.name || removedRuleName)" variant="secondary">
            {{ ruleBaseline.name || removedRuleName }}
          </Badge>
          <Badge v-if="hasUnsavedRule" variant="outline">
            未保存
          </Badge>
        </div>
        <p v-if="editingRuleMissing" role="alert" class="rounded-md border border-destructive/40 p-3 text-sm text-destructive">
          当前正在编辑的告警规则「{{ ruleBaseline.name || removedRuleName || rule.id }}」在最新同步中已不存在，无法继续保存。当前未保存输入仍保留在表单中，请点击“取消编辑”返回新增态。
        </p>
        <p v-else-if="hasUnsavedRule" role="status" class="text-sm text-muted-foreground">
          告警规则表单有未保存修改；仅在切换告警规则、离开栏目或主动刷新时提示，不会影响上方通知渠道草稿。
        </p>
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <label class="grid gap-2 text-sm">规则名称<Input v-model="rule.name" class="h-11" required maxlength="128" /></label>
          <label class="grid gap-2 text-sm">监测条件<SelectControl v-model="rule.metric" aria-label="监测条件" :disabled="busy" :options="metricOptions" /></label>
          <label v-if="rule.metric !== 'offline'" class="grid gap-2 text-sm">{{ thresholdLabel }}<Input v-model="rule.threshold" class="h-11" type="number" min="0" :max="rule.metric === 'expiry' ? 3650 : 100" step="any" required /></label>
          <label class="grid gap-2 text-sm">持续时间（秒）<Input v-model="rule.duration_seconds" class="h-11" type="number" min="0" max="86400" required /></label>
          <label class="grid gap-2 text-sm">通知冷却（秒）<Input v-model="rule.cooldown_seconds" class="h-11" type="number" min="60" max="604800" required /></label>
          <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="rule.enabled" type="checkbox" class="size-4 accent-primary">启用规则</label>
        </fieldset>
        <div class="grid gap-4 sm:grid-cols-2">
          <fieldset :disabled="busy" class="rounded-md border p-3">
            <legend class="px-1 text-sm font-medium">
              监测节点
            </legend>
            <p class="mb-2 text-xs text-muted-foreground">
              不选择时应用于所有活动节点，包括之后注册的节点。
            </p>
            <div class="max-h-64 space-y-2 overflow-auto">
              <label v-for="node in assignableNodes" :key="node.id" class="flex min-h-9 items-center gap-2 text-sm" :class="node.revoked || node.missing ? 'text-destructive' : ''"><input v-model="rule.node_ids" type="checkbox" :value="node.id" class="size-4 accent-primary">{{ node.label }}</label>
            </div>
            <div v-if="needsGlobalConfirmation" class="mt-3 space-y-2 rounded-md border border-destructive/40 p-2 text-xs">
              <p class="text-destructive">
                当前未指定任何节点且将启用此规则。为避免静默扩大告警范围，请选择活动节点、保持停用，或显式确认改为应用于所有活动节点。
              </p>
              <label class="flex min-h-9 items-center gap-2 text-sm"><input v-model="confirmGlobalScope" type="checkbox" class="size-4 accent-primary">确认改为应用于所有活动节点</label>
            </div>
          </fieldset>
          <fieldset :disabled="busy" class="rounded-md border p-3">
            <legend class="px-1 text-sm font-medium">
              通知渠道（可选）
            </legend>
            <p class="mb-2 text-xs text-muted-foreground">
              不选择渠道时，仅记录站内告警事件。
            </p>
            <div class="max-h-64 space-y-2 overflow-auto">
              <label v-for="item in selectableChannels" :key="item.id" class="flex min-h-9 items-center gap-2 text-sm" :class="item.missing ? 'text-destructive' : ''"><input v-model="rule.channel_ids" type="checkbox" :value="item.id" class="size-4 accent-primary">{{ item.label }}</label>
            </div>
            <div v-if="hasMissingChannels" class="mt-3 flex flex-wrap items-center justify-between gap-2 rounded-md border border-destructive/40 p-2 text-xs">
              <span class="text-destructive">草稿包含已失效渠道，请取消勾选或移除后再保存。</span>
              <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="clearMissingChannels">
                移除失效渠道
              </Button>
            </div>
          </fieldset>
        </div>
        <div class="space-y-2">
          <div class="flex flex-wrap items-center gap-2">
            <Button type="submit" class="min-h-11" :disabled="busy || editingRuleMissing">
              {{ ruleBusy ? '正在保存…' : '保存规则' }}
            </Button>
            <Button v-if="showRuleResync" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncRule">
              重新同步
            </Button>
            <Button v-if="rule.id" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="cancelRuleEdit($event)">
              取消编辑
            </Button>
          </div>
          <p v-if="ruleValidationError" role="alert" class="text-sm text-destructive">
            {{ ruleValidationError }}
          </p>
          <p v-if="ruleAlert" role="alert" class="text-sm text-destructive">
            {{ ruleAlert }}
          </p>
          <p v-if="ruleSuccess" role="status" class="text-sm text-success">
            {{ ruleSuccess }}
          </p>
        </div>
      </form>
    </CardX>

    <CardX title="告警事件">
      <p v-if="!incidents.length" class="text-sm text-muted-foreground">
        尚无告警事件。
      </p>
      <div v-else class="max-h-[32rem] overflow-auto">
        <table class="w-full text-left text-sm">
          <caption class="sr-only">
            最近告警事件
          </caption>
          <thead>
            <tr class="border-b">
              <th class="p-2">
                节点
              </th>
              <th class="p-2">
                状态
              </th>
              <th class="p-2">
                说明
              </th>
              <th class="p-2">
                更新时间
              </th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="incident in incidents" :key="incident.id" class="border-b">
              <th class="p-2 font-medium">
                {{ nodeName(incident.node_id) }}
                <p v-if="rules.some(item => item.id === incident.rule_id && item.metric === 'traffic')" class="mt-1 max-w-xs text-xs font-normal text-muted-foreground">
                  {{ trafficSelectionDetails(nodes.find(item => item.id === incident.node_id)?.selected_traffic) }}
                  <span v-if="nodes.find(item => item.id === incident.node_id)?.selected_traffic?.percentage == null"> · 当前不可判定，保留既有事件状态</span>
                </p>
              </th>
              <td class="p-2">
                {{ incidentStatus(incident.status) }}
              </td>
              <td class="p-2">
                {{ incident.message }}
              </td>
              <td class="whitespace-nowrap p-2">
                {{ date(incident.updated_at_unix_ms) }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </CardX>

    <CardX v-if="failures.length" title="最近通知投递失败">
      <ul class="max-h-64 space-y-2 overflow-auto text-sm">
        <li v-for="(failure, index) in failures" :key="index">
          <span class="text-muted-foreground">{{ date(failure.happened_at_unix_ms) }}</span> · {{ failure.subject }}
        </li>
      </ul>
    </CardX>

    <Dialog :open="draftGuardOpen" @update:open="value => { if (!value) cancelDraftGuard() }">
      <DialogContent @close-auto-focus="restoreDraftGuardFocus">
        <DialogHeader>
          <DialogTitle>放弃未保存修改？</DialogTitle>
          <DialogDescription>{{ draftGuardDescription }}</DialogDescription>
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
      <DialogContent :show-close="!busy" @escape-key-down="event => { if (busy) event.preventDefault() }" @interact-outside="event => { if (busy) event.preventDefault() }">
        <DialogHeader>
          <DialogTitle>{{ removingDialogTitle }}</DialogTitle>
          <DialogDescription>确认删除“{{ removingTarget?.name }}”？此操作会影响后续告警通知。</DialogDescription>
        </DialogHeader>
        <p v-if="removingDraftWarning" class="text-sm text-muted-foreground">
          {{ removingDraftWarning }}
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
            {{ removingConfirmLabel }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
