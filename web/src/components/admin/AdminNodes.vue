<script setup lang="ts">
import type { RouteLocationNormalized } from 'vue-router'
import type { ManagedNode } from '@/utils/admin'
import type { NodeDraftForm, NodeDraftSnapshot } from '@/utils/adminDraft'
import type { NodeDangerActionKind, PendingUncertainDangerLock } from '@/utils/adminUncertain'
import type { TagColor } from '@/utils/tagHelper'
import { computed, inject, onBeforeUnmount, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AdminTrafficCorrection from '@/components/admin/AdminTrafficCorrection.vue'
import NodeTagBadge from '@/components/NodeTagBadge.vue'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import SelectControl from '@/components/ui/SelectControl.vue'
import { useAdminAction } from '@/composables/useAdminAction'
import { ADMIN_DRAFT_REGISTRY_KEY, useAdminDraftGuard } from '@/composables/useAdminDraftGuard'
import { useAuthStore } from '@/stores/auth'
import { cloneNodeDraft, createNodeDraftSnapshot, isNodeDraftDirty, shouldSyncNodeDraftFromServer } from '@/utils/adminDraft'
import { normalizePublicRemark, validatePublicRemark } from '@/utils/adminEditing'
import {
  enrollUncertainLock,
  isSameActiveSession,
  nodeDangerUncertainLocks,
  setEnrollUncertainLock,
  setNodeDangerUncertainLock,
} from '@/utils/adminUncertain'
import { getSharedApi } from '@/utils/api'
import { trafficSelectionDetails } from '@/utils/nodeHelpers'
import { getSessionGeneration } from '@/utils/session'
import { parseTags } from '@/utils/tagHelper'

interface PendingUncertainNodeSave {
  nodeId: string
  nodeName: string
  submitted: NodeDraftSnapshot
  message: string
  resyncError: string
}

const props = defineProps<{ nodes: ManagedNode[], refresh: () => Promise<void> }>()
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const draftRegistry = inject(ADMIN_DRAFT_REGISTRY_KEY, null)

const selected = computed(() => props.nodes.find(n => n.id === route.query.node) ?? props.nodes[0])
const draft = ref<NodeDraftForm | null>(null)
const savedBaseline = ref<NodeDraftSnapshot | null>(null)
const loadedNodeId = ref<string | null>(null)
const expires = ref('')
const quotaGiB = ref<number | string>(0)
const ttlHours = ref(24)
const secret = ref('')
const confirm = ref<NodeDangerActionKind | null>(null)
const newTagText = ref('')
const newTagColor = ref<TagColor>('blue')
const tagError = ref('')
const enrollUncertain = enrollUncertainLock
const pendingUncertainSaves = ref<Record<string, PendingUncertainNodeSave>>({})
const pendingUncertainDangers = nodeDangerUncertainLocks
let forceResetNextSync = false

const tagPreview = computed(() => parseTags(draft.value?.tags))
const tagColors: { value: TagColor, label: string }[] = [
  { value: 'gray', label: '灰色' },
  { value: 'green', label: '绿色' },
  { value: 'blue', label: '蓝色' },
  { value: 'violet', label: '紫色' },
  { value: 'amber', label: '琥珀色' },
  { value: 'ruby', label: '红色' },
]
const invalidTagPattern = /[;<>]/
const trailingSemicolonsPattern = /;+$/
const utcTime = (ms: number) => `${new Date(ms).toISOString().replace('T', ' ').slice(0, 19)} UTC`

const enrollAction = useAdminAction()
const saveAction = useAdminAction(props.refresh)
const dangerAction = useAdminAction(props.refresh)

const {
  busy: enrollBusy,
  error: enrollError,
  success: enrollSuccess,
} = enrollAction
const {
  busy: saveBusy,
  error: saveError,
  resyncError: saveResyncError,
  success: saveSuccess,
  needsResync: saveNeedsResync,
} = saveAction
const {
  busy: dangerBusy,
  error: dangerError,
  resyncError: dangerResyncError,
  success: dangerSuccess,
  outcome: dangerOutcome,
  needsResync: dangerNeedsResync,
} = dangerAction

const busy = computed(() => enrollBusy.value || saveBusy.value || dangerBusy.value)
const hasUnsavedChanges = computed(() => isNodeDraftDirty({
  draft: draft.value,
  expires: expires.value,
  quotaGiB: quotaGiB.value,
  newTagText: newTagText.value,
}, savedBaseline.value))

const currentPendingSave = computed(() => {
  const id = selected.value?.id
  if (!id)
    return null
  return pendingUncertainSaves.value[id] ?? null
})

const saveAlert = computed(() => {
  const pending = currentPendingSave.value
  if (pending) {
    return pending.resyncError
      ? `${pending.message}（重新同步失败：${pending.resyncError}）`
      : pending.message
  }
  return saveError.value
})
const showSaveResync = computed(() => saveNeedsResync.value || Boolean(currentPendingSave.value))

const activeConfirmUncertain = computed(() => {
  if (!confirm.value || !selected.value)
    return null
  return pendingUncertainDangers.value[`${confirm.value}:${selected.value.id}`] ?? null
})

const selectedNodePendingDangers = computed(() => {
  const id = selected.value?.id
  if (!id)
    return []
  return Object.values(pendingUncertainDangers.value).filter(item => item.nodeId === id)
})

const showInlineDangerRefreshError = computed(() => {
  if (confirm.value !== null)
    return false
  return dangerOutcome.value === 'refresh_failed' && Boolean(dangerError.value)
})

const actionLabels: Record<NodeDangerActionKind, string> = {
  'rotate': '轮换 Agent 凭据',
  'revoke': '撤销 Agent 凭据',
  'delete': '删除节点',
  'reset-traffic': '重置本周期流量',
}
const actionHints: Record<NodeDangerActionKind, string> = {
  'rotate': '旧凭据会立即失效。请保存新凭据并更新对应 Agent，否则节点将停止上报。',
  'revoke': '对应 Agent 将无法继续上报。需要重新注册后才能恢复。',
  'delete': '将删除此节点及相关数据，无法从面板恢复。',
  'reset-traffic': '将本周期流量计数归零，不会重置主机的网络接口计数。',
}

function formatPendingAlert(message: string, resyncFailure: string): string {
  return resyncFailure
    ? `${message}（重新同步失败：${resyncFailure}）`
    : message
}

function loadDraftFromNode(node: ManagedNode | undefined): void {
  if (!node) {
    draft.value = null
    savedBaseline.value = null
    loadedNodeId.value = null
    expires.value = ''
    quotaGiB.value = 0
    newTagText.value = ''
    tagError.value = ''
    return
  }
  const snapshot = createNodeDraftSnapshot(node)
  draft.value = cloneNodeDraft(snapshot.draft)
  savedBaseline.value = snapshot
  loadedNodeId.value = node.id
  expires.value = snapshot.expires
  quotaGiB.value = snapshot.quotaGiB
  newTagText.value = ''
  tagError.value = ''
}

watch(selected, (node) => {
  const shouldSync = shouldSyncNodeDraftFromServer({
    incomingNodeId: node?.id ?? null,
    loadedNodeId: loadedNodeId.value,
    hasUnsavedChanges: hasUnsavedChanges.value,
    forceReset: forceResetNextSync,
  })
  if (!shouldSync) {
    if (node) {
      const serverSnapshot = createNodeDraftSnapshot(node)
      if (!isNodeDraftDirty({
        draft: draft.value,
        expires: expires.value,
        quotaGiB: quotaGiB.value,
        newTagText: newTagText.value,
      }, serverSnapshot)) {
        savedBaseline.value = serverSnapshot
      }
    }
    return
  }
  forceResetNextSync = false
  loadDraftFromNode(node)
}, { immediate: true })

function isStayingOnSameNode(to: RouteLocationNormalized): boolean {
  if (to.name !== 'admin')
    return false
  const targetSection = typeof to.query.section === 'string' && to.query.section
    ? to.query.section
    : 'nodes'
  if (targetSection !== 'nodes')
    return false
  const targetNodeId = typeof to.query.node === 'string' && to.query.node
    ? to.query.node
    : props.nodes[0]?.id ?? ''
  return targetNodeId === (selected.value?.id ?? '')
}

const draftGuard = useAdminDraftGuard({
  isLoggedIn: () => auth.loggedIn,
  isDirty: () => hasUnsavedChanges.value,
  isBusy: () => busy.value,
  isStayingInSameContext: isStayingOnSameNode,
})
const {
  pending: draftGuardPending,
  isOpen: draftGuardOpen,
  cancel: cancelDraftGuard,
  confirm: confirmDraftGuard,
  restoreFocus: restoreDraftGuardFocus,
} = draftGuard

const unregisterDraft = draftRegistry?.register({
  section: 'nodes',
  isDirty: () => hasUnsavedChanges.value,
  isBusy: () => busy.value,
  requestRefresh: triggerEl => draftGuard.requestRefresh(async () => {
    const activeId = selected.value?.id
    forceResetNextSync = true
    saveAction.clearFeedback()
    try {
      await props.refresh()
      if (activeId && pendingUncertainSaves.value[activeId]) {
        const nextSaves = { ...pendingUncertainSaves.value }
        delete nextSaves[activeId]
        pendingUncertainSaves.value = nextSaves
      }
    }
    catch (cause) {
      forceResetNextSync = false
      saveError.value = cause instanceof Error
        ? cause.message
        : String(cause)
    }
  }, triggerEl),
})

onBeforeUnmount(() => {
  unregisterDraft?.()
})

function addTag(): void {
  if (!draft.value)
    return
  const text = newTagText.value.trim()
  if (!text || invalidTagPattern.test(text)) {
    tagError.value = '请输入标签文字，不能包含分号或尖括号。'
    return
  }
  const tag = `${text}<${newTagColor.value}>`
  const existing = draft.value.tags.trim().replace(trailingSemicolonsPattern, '')
  const next = existing
    ? `${existing};${tag}`
    : tag
  if (new TextEncoder().encode(next).length > 512) {
    tagError.value = '标签总长度不能超过 512 个字符。'
    return
  }
  draft.value.tags = next
  newTagText.value = ''
  tagError.value = ''
}

function applyNodeSwitch(id: string): void {
  secret.value = ''
  saveAction.clearFeedback()
  if (dangerOutcome.value !== 'refresh_failed')
    dangerAction.clearFeedback()
  const targetNode = props.nodes.find(item => item.id === id)
  if (targetNode)
    loadDraftFromNode(targetNode)
  void router.replace({
    query: { ...route.query, node: id },
  })
}

function choose(id: string): void {
  if (id === selected.value?.id || busy.value)
    return
  const targetName = props.nodes.find(item => item.id === id)?.name ?? id
  draftGuard.requestSwitch(
    id,
    targetName,
    () => applyNodeSwitch(id),
    document.getElementById('managed-node'),
  )
}

function openConfirm(action: NodeDangerActionKind): void {
  if (dangerOutcome.value === 'failed' || dangerOutcome.value === 'saved')
    dangerAction.clearFeedback()
  confirm.value = action
}

function closeConfirm(): void {
  if (busy.value)
    return
  confirm.value = null
  if (dangerOutcome.value === 'failed')
    dangerAction.clearFeedback()
}

function acknowledgeEnrollUncertain(): void {
  enrollUncertain.value = null
  enrollAction.clearFeedback()
}

function unlockUncertainDanger(key: string): void {
  const next = { ...pendingUncertainDangers.value }
  delete next[key]
  pendingUncertainDangers.value = next
  if (dangerOutcome.value === 'uncertain')
    dangerAction.clearFeedback()
}

function clearPendingSave(nodeId: string): void {
  if (!pendingUncertainSaves.value[nodeId])
    return
  const next = { ...pendingUncertainSaves.value }
  delete next[nodeId]
  pendingUncertainSaves.value = next
}

async function save(): Promise<void> {
  if (!draft.value)
    return
  const remarkError = validatePublicRemark(draft.value.public_remark)
  if (remarkError) {
    saveError.value = remarkError
    saveSuccess.value = ''
    return
  }
  const normalizedRemark = normalizePublicRemark(draft.value.public_remark)
  const numericQuota = Number(quotaGiB.value) || 0
  const node: NodeDraftForm = {
    ...cloneNodeDraft(draft.value),
    public_remark: normalizedRemark,
    expired_at_unix_ms: expires.value
      ? new Date(expires.value).getTime()
      : null,
    traffic_limit_bytes: Math.round(numericQuota * 1024 ** 3),
  }
  const submittedExpires = expires.value
  const submittedSnapshot: NodeDraftSnapshot = {
    draft: cloneNodeDraft(node),
    expires: submittedExpires,
    quotaGiB: numericQuota,
  }
  clearPendingSave(node.id)
  const result = await saveAction.run(async () => {
    await getSharedApi().post(`admin/nodes/${encodeURIComponent(node.id)}`, node)
  }, {
    message: '已保存',
    onCommitted: () => {
      if (!draft.value || draft.value.id !== node.id)
        return
      draft.value.public_remark = normalizedRemark
      quotaGiB.value = numericQuota
      newTagText.value = ''
      tagError.value = ''
      savedBaseline.value = {
        draft: cloneNodeDraft(draft.value),
        expires: submittedExpires,
        quotaGiB: numericQuota,
      }
    },
  })
  if (result === 'uncertain') {
    pendingUncertainSaves.value = {
      ...pendingUncertainSaves.value,
      [node.id]: {
        nodeId: node.id,
        nodeName: node.name,
        submitted: submittedSnapshot,
        message: saveError.value,
        resyncError: '',
      },
    }
  }
}

async function enroll(): Promise<void> {
  if (busy.value || enrollUncertain.value)
    return
  const sessionGen = getSessionGeneration()
  secret.value = ''
  const result = await enrollAction.run(async () => {
    const response = await getSharedApi().post<{ token: string }>('admin/enrollment', { ttl_seconds: ttlHours.value * 3600 })
    if (isSameActiveSession(sessionGen, auth.loggedIn))
      secret.value = response.token
  }, {
    message: '已生成注册令牌',
    reload: false,
    riskyWrite: true,
    uncertainMessage: detail => `生成一次性注册令牌结果未确认（${detail}）。管理状态接口不展示历史一次性令牌，为避免重复签发已暂停直接重试；如确认仍需新令牌，请先解除暂停后再重新生成。`,
  })
  if (result === 'uncertain')
    setEnrollUncertainLock(sessionGen, auth.loggedIn, enrollError.value)
}

function buildDangerUncertainMessage(action: NodeDangerActionKind, nodeName: string, detail: string): string {
  if (action === 'rotate')
    return `节点「${nodeName}」的轮换 Agent 凭据结果未确认（${detail}）。管理状态接口无法核实凭据是否已轮换；若服务端已生效，旧凭据可能已失效且新令牌未能传回。请先检查对应 Agent 连接状态，或明确解锁后再次轮换以获取新凭据。`
  if (action === 'reset-traffic')
    return `节点「${nodeName}」的重置本周期流量结果未确认（${detail}）。仅凭管理状态接口的当前流量值无法证明重置是否已生效；请先刷新状态核对当前流量，必要时再明确解锁重试。`
  return `节点「${nodeName}」的${actionLabels[action]}结果未确认（${detail}）。请先重新同步核对当前状态，勿直接重复提交。`
}

async function execute(): Promise<void> {
  if (!confirm.value || !selected.value || activeConfirmUncertain.value)
    return
  const sessionGen = getSessionGeneration()
  const action = confirm.value
  const id = selected.value.id
  const nodeName = selected.value.name
  const key = `${action}:${id}`
  secret.value = ''
  const result = await dangerAction.run(async () => {
    if (action === 'rotate') {
      const response = await getSharedApi().post<{ agent_token: string }>(`admin/nodes/${encodeURIComponent(id)}/rotate`)
      if (isSameActiveSession(sessionGen, auth.loggedIn))
        secret.value = response.agent_token
    }
    else {
      await getSharedApi().post(`admin/nodes/${encodeURIComponent(id)}/${action}`)
    }
  }, {
    message: '操作已完成',
    riskyWrite: true,
    uncertainMessage: detail => buildDangerUncertainMessage(action, nodeName, detail),
    onCommitted: () => {
      if (!isSameActiveSession(sessionGen, auth.loggedIn))
        return
      confirm.value = null
      if (pendingUncertainDangers.value[key]) {
        const next = { ...pendingUncertainDangers.value }
        delete next[key]
        pendingUncertainDangers.value = next
      }
    },
  })
  if (result === 'uncertain') {
    setNodeDangerUncertainLock(sessionGen, auth.loggedIn, {
      key,
      action,
      nodeId: id,
      nodeName,
      verifiable: action === 'delete' || action === 'revoke',
      message: dangerError.value,
      resyncError: '',
    })
  }
}

async function resyncSave(): Promise<void> {
  const pending = currentPendingSave.value
  if (!pending) {
    await saveAction.resync('已重新同步最新状态')
    return
  }
  pending.resyncError = ''
  const ok = await saveAction.resync({
    verify: () => {
      const freshNode = props.nodes.find(item => item.id === pending.nodeId)
      clearPendingSave(pending.nodeId)
      if (!freshNode) {
        return {
          resolved: true,
          message: `已核实：节点「${pending.nodeName}」已不存在`,
        }
      }
      const serverSnapshot = createNodeDraftSnapshot(freshNode)
      const applied = !isNodeDraftDirty({
        draft: pending.submitted.draft,
        expires: pending.submitted.expires,
        quotaGiB: pending.submitted.quotaGiB,
        newTagText: '',
      }, serverSnapshot)
      if (applied) {
        let editedAfterSubmit = false
        if (selected.value?.id === pending.nodeId) {
          savedBaseline.value = serverSnapshot
          editedAfterSubmit = isNodeDraftDirty({
            draft: draft.value,
            expires: expires.value,
            quotaGiB: quotaGiB.value,
            newTagText: newTagText.value,
          }, serverSnapshot)
        }
        return {
          resolved: true,
          message: editedAfterSubmit
            ? `已核实：上次提交的节点「${pending.nodeName}」修改已在服务端生效；您之后的新修改已保留，尚未保存。`
            : `已核实：节点「${pending.nodeName}」的修改已在服务端生效`,
        }
      }
      return {
        resolved: true,
        message: `已同步最新状态：服务端节点「${pending.nodeName}」与刚才提交不一致，已保留当前输入，请核对后再决定是否重新保存。`,
      }
    },
  })
  if (!ok && pendingUncertainSaves.value[pending.nodeId])
    pendingUncertainSaves.value[pending.nodeId]!.resyncError = saveResyncError.value
}

async function resyncDanger(entry?: PendingUncertainDangerLock): Promise<void> {
  if (!entry) {
    await dangerAction.resync('已重新同步最新状态')
    return
  }
  const sessionGen = getSessionGeneration()
  entry.resyncError = ''
  const ok = await dangerAction.resync({
    verify: () => {
      if (!isSameActiveSession(sessionGen, auth.loggedIn)) {
        return {
          resolved: false,
          error: '',
        }
      }
      const freshNode = props.nodes.find(item => item.id === entry.nodeId)
      if (entry.action === 'delete') {
        unlockUncertainDanger(entry.key)
        if (!freshNode) {
          if (confirm.value === 'delete')
            confirm.value = null
          return {
            resolved: true,
            message: `已核实：节点「${entry.nodeName}」已删除`,
          }
        }
        return {
          resolved: true,
          message: `已核对最新状态：节点「${entry.nodeName}」仍存在，可确认后再决定是否重试删除。`,
        }
      }
      if (entry.action === 'revoke') {
        unlockUncertainDanger(entry.key)
        if (!freshNode) {
          if (confirm.value === 'revoke')
            confirm.value = null
          return {
            resolved: true,
            message: `已核实：节点「${entry.nodeName}」已不存在`,
          }
        }
        if (freshNode.disabled) {
          if (confirm.value === 'revoke')
            confirm.value = null
          return {
            resolved: true,
            message: `已核实：节点「${entry.nodeName}」的 Agent 凭据已撤销`,
          }
        }
        return {
          resolved: true,
          message: `已核对最新状态：节点「${entry.nodeName}」的 Agent 凭据仍处于有效状态，可确认后再决定是否重试撤销。`,
        }
      }
      const honestMessage = entry.action === 'rotate'
        ? `已刷新管理状态，但该接口无法证明节点「${entry.nodeName}」的凭据轮换是否已生效；请结合 Agent 连接状态核对后再决定是否解锁重新轮换。`
        : `已刷新管理状态，但仅凭当前流量值无法证明节点「${entry.nodeName}」的流量重置是否已生效；请核对当前流量后再决定是否解锁重试。`
      if (pendingUncertainDangers.value[entry.key]) {
        setNodeDangerUncertainLock(sessionGen, auth.loggedIn, {
          ...pendingUncertainDangers.value[entry.key]!,
          message: honestMessage,
          resyncError: '',
        })
      }
      return {
        resolved: false,
        error: honestMessage,
      }
    },
  })
  if (!ok && dangerResyncError.value && isSameActiveSession(sessionGen, auth.loggedIn) && pendingUncertainDangers.value[entry.key])
    pendingUncertainDangers.value[entry.key]!.resyncError = dangerResyncError.value
}
</script>

<template>
  <div class="space-y-4">
    <CardX title="接入节点" content-class="space-y-4">
      <form class="flex flex-wrap items-end gap-3" @submit.prevent="enroll">
        <label class="grid gap-2 text-sm">注册令牌有效期（小时）<Input v-model="ttlHours" type="number" min="1" max="24" required class="max-w-48" :disabled="busy" /></label>
        <Button type="submit" class="min-h-11" :disabled="busy || Boolean(enrollUncertain)">
          {{ enrollBusy ? '正在生成…' : '生成一次性注册令牌' }}
        </Button>
      </form>
      <p class="text-sm text-muted-foreground">
        令牌仅用于注册一个 Agent，请通过受保护的配置文件交给对应服务器。
      </p>
      <div v-if="enrollUncertain" class="flex flex-wrap items-center gap-2">
        <p role="alert" class="text-sm text-destructive">
          {{ enrollUncertain }}
        </p>
        <Button type="button" variant="outline" size="sm" :disabled="busy" @click="acknowledgeEnrollUncertain">
          已知悉，允许重新生成
        </Button>
      </div>
      <p v-else-if="enrollError" role="alert" class="text-sm text-destructive">
        {{ enrollError }}
      </p>
      <p v-if="enrollSuccess" role="status" class="text-sm text-success">
        {{ enrollSuccess }}
      </p>
      <div v-if="secret" class="space-y-2 rounded-md border p-3">
        <label for="agent-secret" class="text-sm font-medium">新令牌（离开页面后清除）</label>
        <Input id="agent-secret" :model-value="secret" readonly autocomplete="off" class="font-mono" @focus="($event.target as HTMLInputElement).select()" />
        <Button variant="outline" type="button" @click="secret = ''">
          隐藏令牌
        </Button>
      </div>
    </CardX>
    <CardX title="节点管理" content-class="space-y-4">
      <label v-if="nodes.length" for="managed-node" class="grid gap-2 text-sm">选择节点
        <SelectControl id="managed-node" aria-label="选择节点" :model-value="selected?.id ?? ''" :disabled="busy" :options="nodes.map(node => ({ value: node.id, label: `${node.name} · ${node.id.slice(0, 8)}${node.hidden ? ' · 已隐藏' : ''}${node.disabled ? ' · 凭据已撤销' : ''}` }))" @update:model-value="choose" />
      </label>
      <p v-else class="text-sm text-muted-foreground">
        尚无节点。生成注册令牌并安装 Agent 后，节点会出现在这里。
      </p>
      <p v-if="hasUnsavedChanges" role="status" class="text-sm text-muted-foreground">
        有未保存修改；后台状态刷新不会覆盖未保存的节点输入。
      </p>
      <form v-if="draft" class="space-y-5" :aria-busy="busy" @submit.prevent="save">
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <legend class="mb-3 font-medium">
            显示与分组
          </legend>
          <label class="grid gap-2 text-sm">名称<Input v-model="draft.name" required maxlength="128" /></label>
          <label class="grid gap-2 text-sm">地区代码<Input v-model="draft.region" placeholder="例如 SG" maxlength="16" /></label>
          <label class="grid gap-2 text-sm">公网 IPv4（可选）<Input v-model="draft.ipv4" maxlength="15" aria-describedby="node-ip-privacy" /></label>
          <label class="grid gap-2 text-sm">公网 IPv6（可选）<Input v-model="draft.ipv6" maxlength="45" aria-describedby="node-ip-privacy" /></label>
          <p id="node-ip-privacy" class="text-sm text-muted-foreground sm:col-span-2 lg:col-span-3">
            登记地址用于节点互测，启用 IP 信息查询后也用于 LuminaPlus。探测 Agent 仅接收分配的地址，互测面板不显示地址。
          </p>
          <label class="grid gap-2 text-sm">节点互测协议
            <SelectControl v-model="draft.peer_probe.kind" aria-label="节点互测协议" :disabled="busy || draft.peer_probe.disabled" :options="[{ value: 'tcp', label: 'TCP 443' }, { value: 'icmp', label: 'ICMP Ping' }]" />
          </label>
          <label class="grid gap-2 text-sm">节点互测地址族
            <SelectControl v-model="draft.peer_probe.family" aria-label="节点互测地址族" :disabled="busy || draft.peer_probe.disabled" :options="[{ value: 'auto', label: '自动（优先 IPv4）' }, { value: 'ipv4', label: 'IPv4' }, { value: 'ipv6', label: 'IPv6' }]" />
          </label>
          <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="draft.peer_probe.disabled" type="checkbox" class="size-4 accent-primary">停用此节点的入向互测</label>
          <label class="grid gap-2 text-sm">分组<Input v-model="draft.group" maxlength="128" /></label>
          <label class="grid gap-2 text-sm">排序权重<Input v-model="draft.weight" type="number" min="-2147483648" max="2147483647" step="1" required /></label>
          <div class="grid gap-2 text-sm sm:col-span-2 lg:col-span-3">
            <label for="node-tags">小面板标签</label>
            <Input id="node-tags" v-model="draft.tags" placeholder="马来西亚<gray>;V4<green>;V6<green>;带宽1G<violet>" maxlength="512" aria-describedby="node-tags-help" />
            <p id="node-tags-help" class="text-muted-foreground">
              可添加地区、线路、协议和带宽等信息；用分号分隔，颜色格式为 标签文字&lt;颜色英文名&gt;。也可以用下方按钮添加。
            </p>
            <div class="flex flex-wrap items-end gap-2">
              <label class="grid min-w-36 flex-1 gap-1">标签文字<Input v-model="newTagText" maxlength="40" placeholder="例如：带宽1G" @keydown.enter.prevent="addTag" /></label>
              <label class="grid gap-1">颜色
                <SelectControl v-model="newTagColor" aria-label="颜色" :disabled="busy" :options="tagColors" />
              </label>
              <Button type="button" variant="outline" class="min-h-11" @click="addTag">
                添加标签
              </Button>
            </div>
            <p v-if="tagError" role="alert" class="text-destructive">
              {{ tagError }}
            </p>
            <div v-if="tagPreview.length" class="flex flex-wrap gap-1.5 rounded-md border border-border/60 p-2" aria-label="标签预览">
              <NodeTagBadge v-for="(tag, index) in tagPreview" :key="index" :text="tag.text" :color="tag.color" />
            </div>
          </div>
          <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="draft.hidden" type="checkbox" class="size-4 accent-primary">从监控视图隐藏（管理列表保留）</label>
          <label class="grid gap-2 text-sm sm:col-span-2 lg:col-span-3">公开备注<textarea v-model="draft.public_remark" rows="3" maxlength="2048" class="rounded-md border border-input bg-background px-3 py-2 focus-visible:ring-2 focus-visible:ring-ring" /></label>
        </fieldset>
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <legend class="mb-3 font-medium">
            费用与到期
          </legend>
          <label class="grid gap-2 text-sm">每周期价格<Input v-model="draft.price" type="number" min="0" max="1000000000" step="0.01" required /></label>
          <label class="grid gap-2 text-sm">币种（大写三字母）<Input v-model="draft.currency" placeholder="USD" pattern="[A-Z]{3}" minlength="3" maxlength="3" required /></label>
          <label class="grid gap-2 text-sm">计费周期（天）<Input v-model="draft.billing_cycle_days" type="number" min="0" max="36500" step="1" required /></label>
          <label class="grid gap-2 text-sm">到期时间（本地时间）<Input v-model="expires" type="datetime-local" /></label>
          <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="draft.auto_renewal" type="checkbox" class="size-4 accent-primary">自动续费标记</label>
          <p class="text-sm text-muted-foreground sm:col-span-2 lg:col-span-3">
            这些信息用于资产记录和提醒，Senlun 不会执行支付或扣款。
          </p>
        </fieldset>
        <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-3">
          <legend class="mb-3 font-medium">
            流量周期
          </legend>
          <label class="grid gap-2 text-sm sm:col-span-3">卡片、列表及额度告警的数据来源
            <SelectControl aria-label="流量数据来源" :model-value="draft.traffic_source ?? 'local'" :disabled="busy" :options="[{ value: 'local', label: '本地网卡周期流量' }, { value: 'provider', label: '服务商记录流量' }]" @update:model-value="draft.traffic_source = $event" />
          </label>
          <p class="text-sm text-muted-foreground sm:col-span-3">
            保存后同时应用于此节点的流量显示与额度告警。服务商未配置、过期或失败时不会切回本地；下列额度、计数方式及重置日只用于本地统计。切换会结束旧来源的阈值评估，不发送恢复通知。
          </p>
          <p v-if="selected" role="status" class="text-sm text-muted-foreground sm:col-span-3">
            当前流量状态：{{ trafficSelectionDetails(selected.selected_traffic) }}
          </p>
          <label class="grid gap-2 text-sm">每周期额度（GiB；0 为不限）<Input v-model="quotaGiB" type="number" min="0" :max="Number.MAX_SAFE_INTEGER / 1024 ** 3" step="any" required /></label>
          <label class="grid gap-2 text-sm">流量计数方式<SelectControl v-model="draft.traffic_limit_type" aria-label="流量计数方式" :disabled="busy" :options="[{ value: 'sum', label: '上传 + 下载' }, { value: 'max', label: '上传、下载中较大值' }, { value: 'min', label: '上传、下载中较小值' }, { value: 'up', label: '仅上传' }, { value: 'down', label: '仅下载' }]" /></label>
          <label class="grid gap-2 text-sm">每月重置日（1–31，UTC）<Input v-model="draft.traffic_reset_day" type="number" min="1" max="31" step="1" required /></label>
          <p class="text-sm text-muted-foreground sm:col-span-3">
            按 UTC 00:00 重置；当月没有所选日期时，在该月最后一天重置。新日期立即生效，现有计数保留至下一个新日期。流量是近似统计，账单以服务商为准。
          </p>
          <p v-if="selected?.traffic_next_reset_unix_ms != null" class="text-sm text-muted-foreground sm:col-span-3">
            下次重置：{{ utcTime(selected.traffic_next_reset_unix_ms) }}
          </p>
        </fieldset>
        <div class="flex flex-wrap gap-2">
          <Button class="min-h-11" type="submit" :disabled="busy">
            {{ saveBusy ? '正在处理…' : '保存节点' }}
          </Button><Button variant="outline" type="button" class="min-h-11" :disabled="selected?.hidden" @click="router.push(`/instance/${draft.id}`)">
            查看监控
          </Button>
          <Button v-if="showSaveResync" variant="outline" type="button" class="min-h-11" :disabled="busy" @click="resyncSave">
            重新同步
          </Button>
        </div>
        <p v-if="saveAlert" role="alert" class="text-sm text-destructive">
          {{ saveAlert }}
        </p>
        <p v-if="saveSuccess" role="status" class="text-sm text-success">
          {{ saveSuccess }}
        </p>
        <p v-if="selected?.hidden" class="text-sm text-muted-foreground">
          此节点已从监控页面和探测历史中隐藏。取消隐藏并保存后可查看。
        </p>
      </form>
      <div v-if="draft" class="space-y-3 border-t pt-4">
        <div class="flex flex-wrap gap-2">
          <AdminTrafficCorrection v-if="selected" :node="selected" :disabled="busy" :refresh="props.refresh" />
          <Button v-for="(label, action) in actionLabels" :key="action" variant="outline" type="button" :disabled="busy" :class="action === 'delete' ? 'text-destructive' : ''" @click="openConfirm(action)">
            {{ label }}
          </Button>
        </div>
        <div v-if="confirm === null && selectedNodePendingDangers.length" class="space-y-2">
          <div v-for="entry in selectedNodePendingDangers" :key="entry.key" class="flex flex-wrap items-center gap-2 rounded-md border border-destructive/40 p-3">
            <p role="alert" class="text-sm text-destructive">
              {{ formatPendingAlert(entry.message, entry.resyncError) }}
            </p>
            <Button v-if="entry.verifiable" type="button" variant="outline" size="sm" :disabled="busy" @click="resyncDanger(entry)">
              重新同步
            </Button>
            <template v-else>
              <Button type="button" variant="outline" size="sm" :disabled="busy" @click="resyncDanger(entry)">
                刷新管理状态
              </Button>
              <Button type="button" variant="outline" size="sm" :disabled="busy" @click="unlockUncertainDanger(entry.key)">
                我已核对状态，解除锁定
              </Button>
            </template>
          </div>
        </div>
        <div v-if="showInlineDangerRefreshError" class="flex flex-wrap items-center gap-2">
          <p role="alert" class="text-sm text-destructive">
            {{ dangerError }}
          </p>
          <Button v-if="dangerNeedsResync" type="button" variant="outline" size="sm" :disabled="busy" @click="resyncDanger()">
            重新同步
          </Button>
        </div>
        <p v-if="confirm === null && dangerSuccess" role="status" class="text-sm text-success">
          {{ dangerSuccess }}
        </p>
      </div>
    </CardX>
    <Dialog :open="draftGuardOpen" @update:open="value => { if (!value) cancelDraftGuard() }">
      <DialogContent @close-auto-focus="restoreDraftGuardFocus">
        <DialogHeader>
          <DialogTitle>放弃未保存修改？</DialogTitle>
          <DialogDescription v-if="draftGuardPending?.kind === 'switch'">
            切换到 {{ draftGuardPending.targetLabel }} 会丢弃当前节点未保存的修改，不会保存任何更改。
          </DialogDescription>
          <DialogDescription v-else-if="draftGuardPending?.kind === 'refresh'">
            刷新管理数据会丢弃当前节点未保存的修改，不会保存任何更改。
          </DialogDescription>
          <DialogDescription v-else>
            离开当前节点编辑会丢弃未保存的修改，不会保存任何更改。
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" @click="cancelDraftGuard">
            取消，继续编辑
          </Button>
          <Button variant="destructive" @click="confirmDraftGuard">
            {{
              draftGuardPending?.kind === 'switch'
                ? '放弃修改并切换'
                : draftGuardPending?.kind === 'refresh'
                  ? '放弃修改并刷新'
                  : '放弃修改并离开'
            }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
    <Dialog :open="confirm !== null" @update:open="value => { if (!value) closeConfirm() }">
      <DialogContent :show-close="!busy" @escape-key-down="event => { if (busy) event.preventDefault() }" @interact-outside="event => { if (busy) event.preventDefault() }">
        <DialogHeader><DialogTitle>{{ confirm ? actionLabels[confirm] : '' }}</DialogTitle><DialogDescription>{{ selected?.name }}：{{ confirm ? actionHints[confirm] : '' }}</DialogDescription></DialogHeader>
        <p v-if="activeConfirmUncertain" role="alert" class="text-sm text-destructive">
          {{ formatPendingAlert(activeConfirmUncertain.message, activeConfirmUncertain.resyncError) }}
        </p>
        <p v-else-if="dangerError" role="alert" class="text-sm text-destructive">
          {{ dangerError }}
        </p>
        <p v-if="!activeConfirmUncertain && dangerSuccess" role="status" class="text-sm text-success">
          {{ dangerSuccess }}
        </p>
        <DialogFooter>
          <Button variant="outline" :disabled="busy" @click="closeConfirm">
            取消
          </Button>
          <template v-if="activeConfirmUncertain">
            <Button v-if="activeConfirmUncertain.verifiable" variant="outline" :disabled="busy" @click="resyncDanger(activeConfirmUncertain)">
              重新同步
            </Button>
            <template v-else>
              <Button variant="outline" :disabled="busy" @click="resyncDanger(activeConfirmUncertain)">
                刷新管理状态
              </Button>
              <Button variant="outline" :disabled="busy" @click="unlockUncertainDanger(activeConfirmUncertain.key)">
                我已核对状态，解除锁定
              </Button>
            </template>
          </template>
          <Button variant="destructive" :disabled="busy || Boolean(activeConfirmUncertain)" @click="execute">
            {{ dangerBusy ? '正在处理…' : '确认执行' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
