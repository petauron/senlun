<script setup lang="ts">
import type { ComponentPublicInstance } from 'vue'
import type { RouteLocationNormalized } from 'vue-router'
import type { ManagedNode } from '@/utils/admin'
import type { NodeDraftForm, NodeDraftSnapshot } from '@/utils/adminDraft'
import type { NodeListFilter } from '@/utils/adminNodes'
import type { NodeDangerActionKind, PendingUncertainDangerLock } from '@/utils/adminUncertain'
import type { TagColor } from '@/utils/tagHelper'
import { Icon } from '@iconify/vue'
import { computed, inject, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AdminTrafficCorrection from '@/components/admin/AdminTrafficCorrection.vue'
import NodeTagBadge from '@/components/NodeTagBadge.vue'
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
  cloneNodeDraft,
  createNodeDraftSnapshot,
  formatNodeDraftBackup,
  isNodeDraftDirty,
  shouldSyncNodeDraftFromServer,
} from '@/utils/adminDraft'
import { normalizePublicRemark, validatePublicRemark } from '@/utils/adminEditing'
import {
  filterManagedNodes,
  NODE_DANGER_ACTION_ORDER,
  NODE_DANGER_ACTIONS,
  NODE_LIST_FILTER_OPTIONS,
  resolveNodeSelection,
} from '@/utils/adminNodes'
import {
  adminNodesViewState,
  enrollUncertainLock,
  isSameActiveSession,
  nodeDangerUncertainLocks,
  setEnrollUncertainLock,
  setNodeDangerUncertainLock,
} from '@/utils/adminUncertain'
import { ApiError, getSharedApi } from '@/utils/api'
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

interface PendingDangerConfirm {
  action: NodeDangerActionKind
  nodeId: string
  nodeName: string
}

interface IssuedSecretNotice {
  token: string
  kind: 'enroll' | 'rotate'
  nodeId?: string
  nodeName?: string
}

const props = defineProps<{ nodes: ManagedNode[], refresh: () => Promise<void> }>()
const emit = defineEmits<{ nodeDeleted: [id: string] }>()
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const draftRegistry = inject(ADMIN_DRAFT_REGISTRY_KEY, null)

const isDesktop = ref(typeof window !== 'undefined' && typeof window.matchMedia === 'function'
  ? window.matchMedia('(min-width: 1024px)').matches
  : true)
let desktopMediaQuery: MediaQueryList | null = null

function handleViewportChange(event: MediaQueryListEvent): void {
  isDesktop.value = event.matches
}

const searchQuery = ref(adminNodesViewState.value.searchQuery)
const statusFilter = ref<NodeListFilter>(adminNodesViewState.value.statusFilter)
const savedListScrollY = ref<number | null>(adminNodesViewState.value.listScrollY)
const lastInteractedNodeId = ref<string | null>(adminNodesViewState.value.lastNodeId)
const editorHeadingRef = ref<HTMLElement | null>(null)
const searchInputRef = ref<HTMLElement | null>(null)
const nodeListEl = ref<HTMLElement | null>(null)
const nodeButtonEls = new Map<string, HTMLButtonElement>()

function setNodeButtonRef(id: string, el: Element | ComponentPublicInstance | null): void {
  if (el instanceof HTMLButtonElement)
    nodeButtonEls.set(id, el)
  else
    nodeButtonEls.delete(id)
}

function setSearchInputRef(el: Element | ComponentPublicInstance | null): void {
  if (el instanceof HTMLElement)
    searchInputRef.value = el
  else if (el && '$el' in el && el.$el instanceof HTMLElement)
    searchInputRef.value = el.$el
  else
    searchInputRef.value = null
}

const queryNodeId = computed(() => typeof route.query.node === 'string'
  ? route.query.node.trim()
  : '')

function initialPinnedNodeId(): string | null {
  if (queryNodeId.value)
    return queryNodeId.value
  if (!isDesktop.value)
    return null
  const remembered = adminNodesViewState.value.lastNodeId
  if (remembered && props.nodes.some(node => node.id === remembered))
    return remembered
  return props.nodes[0]?.id ?? null
}

const pinnedNodeId = ref<string | null>(initialPinnedNodeId())

// When mobile navigates back from ?node=<id> to /admin?section=nodes, clear pinnedNodeId synchronously after route commit.
watch(queryNodeId, (next, prev) => {
  if (prev && !next && !isDesktop.value)
    pinnedNodeId.value = null
}, { flush: 'sync' })

watch(
  [searchQuery, statusFilter],
  ([nextQuery, nextFilter]) => {
    adminNodesViewState.value = {
      ...adminNodesViewState.value,
      searchQuery: nextQuery,
      statusFilter: nextFilter,
    }
  },
)

const selection = computed(() => resolveNodeSelection({
  nodes: props.nodes,
  queryNodeId: queryNodeId.value || null,
  pinnedNodeId: pinnedNodeId.value,
  isDesktop: isDesktop.value,
}))

const selected = computed(() => selection.value.mode === 'selected'
  ? selection.value.node
  : undefined)

const missingRequestedId = computed(() => selection.value.mode === 'missing'
  ? selection.value.requestedId
  : '')

const revokedGuidanceText = computed(() => isDesktop.value
  ? '此节点的 Agent 凭据已撤销，无法再轮换或重复撤销；如需重新接入，请展开上方“接入节点”生成一次性注册令牌以注册独立新节点。'
  : '此节点的 Agent 凭据已撤销，无法再轮换或重复撤销；如需重新接入，可点击下方按钮打开接入节点面板，或返回节点列表展开“接入节点”生成一次性注册令牌以注册独立新节点。')

const filteredNodes = computed(() => filterManagedNodes(props.nodes, searchQuery.value, statusFilter.value))

const hasActiveFilter = computed(() => Boolean(searchQuery.value.trim()) || statusFilter.value !== 'all')

const isSelectedFilteredOut = computed(() => {
  if (!selected.value || !hasActiveFilter.value)
    return false
  return !filteredNodes.value.some(node => node.id === selected.value!.id)
})

const showNodeList = computed(() => isDesktop.value || selection.value.mode === 'list')
const showEditorPane = computed(() => isDesktop.value || selection.value.mode !== 'list')

const draft = ref<NodeDraftForm | null>(null)
const savedBaseline = ref<NodeDraftSnapshot | null>(null)
const loadedNodeId = ref<string | null>(null)
const removedNodeLabel = ref<{ id: string, name: string } | null>(null)
const expires = ref('')
const quotaGiB = ref<number | string>(0)
const ttlHours = ref(24)
const issuedSecret = ref<IssuedSecretNotice | null>(null)
const confirmTarget = ref<PendingDangerConfirm | null>(null)
const newTagText = ref('')
const newTagColor = ref<TagColor>('blue')
const tagError = ref('')
const pageStatusNotice = ref('')
const lastSavedNodeSummary = ref<{ nodeId: string, nodeName: string } | null>(null)
const enrollUncertain = enrollUncertainLock
const enrollExpanded = ref(props.nodes.length === 0 || Boolean(enrollUncertain.value))
const pendingUncertainSaves = ref<Record<string, PendingUncertainNodeSave>>({})
const pendingUncertainDangers = nodeDangerUncertainLocks
let forceResetNextSync = false

const disappearedNode = computed<{ id: string, name: string } | null>(() => {
  if (selection.value.mode === 'removed') {
    const id = selection.value.removedId
    const name = (loadedNodeId.value === id && draft.value?.name)
      || (removedNodeLabel.value?.id === id && removedNodeLabel.value.name)
      || id
    return { id, name }
  }
  if (selection.value.mode === 'missing') {
    const id = selection.value.requestedId
    if (loadedNodeId.value === id)
      return { id, name: draft.value?.name || id }
    if (removedNodeLabel.value?.id === id)
      return { id, name: removedNodeLabel.value.name }
  }
  return null
})

const removedDraftBackup = computed(() => formatNodeDraftBackup({
  draft: draft.value,
  expires: expires.value,
  quotaGiB: quotaGiB.value,
  newTagText: newTagText.value,
}, newTagColor.value))

watch(
  [
    () => props.nodes.length,
    enrollUncertain,
  ],
  ([count, uncertain]) => {
    if (count === 0 || uncertain)
      enrollExpanded.value = true
  },
)

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

const activeConfirmMeta = computed(() => confirmTarget.value
  ? NODE_DANGER_ACTIONS[confirmTarget.value.action]
  : null)

const activeConfirmDescription = computed(() => {
  if (!activeConfirmMeta.value || !confirmTarget.value)
    return ''
  return activeConfirmMeta.value.description(confirmTarget.value.nodeName)
})

const issuedSecretAttribution = computed(() => {
  const secret = issuedSecret.value
  if (!secret)
    return ''
  if (secret.kind === 'rotate')
    return `归属：节点「${secret.nodeName}」轮换后的新 Agent 凭据`
  return '归属：一次性注册令牌（用于注册独立新节点）'
})

const activeConfirmUncertain = computed(() => {
  const target = confirmTarget.value
  if (!target)
    return null
  return pendingUncertainDangers.value[`${target.action}:${target.nodeId}`] ?? null
})

const confirmTargetInvalidReason = computed(() => {
  const target = confirmTarget.value
  if (!target)
    return ''
  const liveNode = props.nodes.find(node => node.id === target.nodeId)
  if (!liveNode)
    return `节点「${target.nodeName}」已不存在（可能已在最新同步中被删除），本次确认已失效，无法执行。`
  if (selected.value?.id !== target.nodeId)
    return `当前编辑对象已切换，不再指向节点「${target.nodeName}」，本次确认已失效，请关闭后重新核对。`
  if (target.action === 'rotate' && liveNode.disabled)
    return `节点「${target.nodeName}」的 Agent 凭据已撤销，无法轮换旧凭据；如需重新接入，请生成一次性注册令牌注册为独立新节点。`
  if (target.action === 'revoke' && liveNode.disabled)
    return `节点「${target.nodeName}」的 Agent 凭据已处于撤销状态，无需重复执行。`
  return ''
})

const confirmButtonLabel = computed(() => {
  if (dangerBusy.value)
    return '正在处理…'
  return activeConfirmMeta.value?.confirmLabel ?? '确认'
})

const selectedNodePendingDangers = computed(() => {
  const id = selected.value?.id
  if (!id)
    return []
  return Object.values(pendingUncertainDangers.value).filter(item => item.nodeId === id)
})

const showTopDangerRefreshAlert = computed(() => {
  if (confirmTarget.value !== null)
    return false
  return dangerOutcome.value === 'refresh_failed' && Boolean(dangerError.value)
})

const topStatusMessage = computed(() => {
  if (pageStatusNotice.value)
    return pageStatusNotice.value
  if (selection.value.mode === 'list' && lastSavedNodeSummary.value)
    return `已保存节点「${lastSavedNodeSummary.value.nodeName}」`
  if (confirmTarget.value === null && selection.value.mode !== 'selected' && dangerSuccess.value)
    return dangerSuccess.value
  return ''
})

const dangerActionsList = NODE_DANGER_ACTION_ORDER.map(action => NODE_DANGER_ACTIONS[action])

function formatPendingAlert(message: string, resyncFailure: string): string {
  return resyncFailure
    ? `${message}（重新同步失败：${resyncFailure}）`
    : message
}

function isDangerActionDisabled(action: NodeDangerActionKind): boolean {
  if (busy.value)
    return true
  if (selected.value?.disabled && (action === 'rotate' || action === 'revoke'))
    return true
  return false
}

function isNodeHighlighted(id: string): boolean {
  if (selected.value)
    return selected.value.id === id
  return lastInteractedNodeId.value === id
}

function resetFilters(): void {
  searchQuery.value = ''
  statusFilter.value = 'all'
}

function clearFiltersAndFocusSearch(): void {
  resetFilters()
  void nextTick(() => {
    searchInputRef.value?.focus({ preventScroll: true })
  })
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

watch(selected, (node, prevNode) => {
  const nextId = node?.id ?? null
  const prevId = prevNode?.id ?? null
  if (node) {
    pinnedNodeId.value = node.id
    removedNodeLabel.value = null
    lastInteractedNodeId.value = node.id
    adminNodesViewState.value = {
      ...adminNodesViewState.value,
      lastNodeId: node.id,
    }
  }
  if (nextId !== prevId) {
    if (issuedSecret.value?.kind === 'rotate' && issuedSecret.value.nodeId !== nextId)
      issuedSecret.value = null
    if (prevId !== null) {
      saveAction.clearFeedback()
      if (dangerOutcome.value !== 'refresh_failed')
        dangerAction.clearFeedback()
    }
  }
  // Preserve draft snapshot read-only when the currently loaded/pinned node disappears concurrently.
  if (!node && disappearedNode.value) {
    removedNodeLabel.value = {
      id: disappearedNode.value.id,
      name: prevNode?.name || disappearedNode.value.name,
    }
    return
  }
  const shouldSync = shouldSyncNodeDraftFromServer({
    incomingNodeId: nextId,
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

async function restoreListScrollAndFocus(): Promise<void> {
  await nextTick()
  if (isDesktop.value) {
    if (nodeListEl.value && adminNodesViewState.value.listScrollTop !== null)
      nodeListEl.value.scrollTop = adminNodesViewState.value.listScrollTop
    return
  }
  const scrollY = savedListScrollY.value ?? adminNodesViewState.value.listScrollY
  if (scrollY !== null && typeof window !== 'undefined')
    window.scrollTo({ top: scrollY, behavior: 'auto' })
  const targetId = lastInteractedNodeId.value
  const targetButton = targetId
    ? nodeButtonEls.get(targetId)
    : undefined
  if (targetButton?.isConnected)
    targetButton.focus({ preventScroll: true })
  else
    searchInputRef.value?.focus({ preventScroll: true })
}

watch(
  () => selection.value.mode,
  async (mode, prevMode) => {
    if (isDesktop.value)
      return
    if (mode === 'list' && (prevMode === 'selected' || prevMode === 'missing' || prevMode === 'removed')) {
      await restoreListScrollAndFocus()
      return
    }
    if (mode === 'selected' && prevMode === 'list') {
      await nextTick()
      if (typeof window !== 'undefined')
        window.scrollTo({ top: 0, behavior: 'auto' })
      editorHeadingRef.value?.focus({ preventScroll: true })
    }
  },
)

onMounted(() => {
  if (typeof window !== 'undefined' && typeof window.matchMedia === 'function') {
    desktopMediaQuery = window.matchMedia('(min-width: 1024px)')
    isDesktop.value = desktopMediaQuery.matches
    desktopMediaQuery.addEventListener('change', handleViewportChange)
  }
  if (showNodeList.value && (adminNodesViewState.value.listScrollY !== null || adminNodesViewState.value.listScrollTop !== null))
    void restoreListScrollAndFocus()
})

function isStayingOnSameNode(to: RouteLocationNormalized): boolean {
  if (to.name !== 'admin')
    return false
  const targetSection = typeof to.query.section === 'string' && to.query.section
    ? to.query.section
    : 'nodes'
  if (targetSection !== 'nodes')
    return false
  const nextQueryNodeId = typeof to.query.node === 'string'
    ? to.query.node.trim()
    : ''
  const nextPinned = nextQueryNodeId || isDesktop.value
    ? pinnedNodeId.value
    : null
  const nextSelection = resolveNodeSelection({
    nodes: props.nodes,
    queryNodeId: nextQueryNodeId || null,
    pinnedNodeId: nextPinned,
    isDesktop: isDesktop.value,
  })
  const currentNodeId = selected.value?.id ?? null
  const nextNodeId = nextSelection.mode === 'selected'
    ? nextSelection.node.id
    : null
  if (currentNodeId === null && nextNodeId === null)
    return nextSelection.mode === selection.value.mode
  return currentNodeId === nextNodeId
}

function describeRouteTarget(to: RouteLocationNormalized): string | undefined {
  if (to.name !== 'admin')
    return undefined
  const targetSection = typeof to.query.section === 'string' && to.query.section
    ? to.query.section
    : 'nodes'
  if (targetSection !== 'nodes')
    return undefined
  const targetId = typeof to.query.node === 'string'
    ? to.query.node.trim()
    : ''
  if (!targetId)
    return undefined
  return props.nodes.find(node => node.id === targetId)?.name
}

const draftGuard = useAdminDraftGuard({
  isLoggedIn: () => auth.loggedIn,
  isDirty: () => hasUnsavedChanges.value,
  isBusy: () => busy.value,
  isStayingInSameContext: isStayingOnSameNode,
  describeRouteTarget,
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
  if (!action)
    return ''
  if (action.kind === 'switch')
    return action.targetLabel
  if (action.kind === 'leave' && action.label)
    return action.label
  return ''
})

const draftGuardConfirmLabel = computed(() => {
  const action = draftGuardPending.value
  if (!action)
    return '放弃修改并离开'
  if (action.kind === 'refresh')
    return '放弃修改并刷新'
  if (action.kind === 'switch' || (action.kind === 'leave' && action.label))
    return '放弃修改并切换'
  return '放弃修改并离开'
})

const unregisterDraft = draftRegistry?.register({
  section: 'nodes',
  isDirty: () => hasUnsavedChanges.value,
  isBusy: () => busy.value,
  requestRefresh: triggerEl => draftGuard.requestRefresh(async () => {
    const activeId = selected.value?.id
    const removedId = disappearedNode.value?.id ?? null
    forceResetNextSync = true
    saveAction.clearFeedback()
    pageStatusNotice.value = ''
    try {
      await props.refresh()
      if (activeId && pendingUncertainSaves.value[activeId]) {
        const nextSaves = { ...pendingUncertainSaves.value }
        delete nextSaves[activeId]
        pendingUncertainSaves.value = nextSaves
      }
      if (removedId && disappearedNode.value?.id === removedId)
        loadDraftFromNode(undefined)
    }
    catch (cause) {
      saveError.value = cause instanceof Error
        ? cause.message
        : String(cause)
    }
    finally {
      forceResetNextSync = false
    }
  }, triggerEl),
})

onBeforeUnmount(() => {
  desktopMediaQuery?.removeEventListener('change', handleViewportChange)
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

async function clearNodeQuery(): Promise<void> {
  const nextQuery = { ...route.query }
  delete nextQuery.node
  await router.replace({
    query: {
      ...nextQuery,
      section: 'nodes',
    },
  })
}

function isNodesListHistoryEntry(backPath: unknown): boolean {
  if (typeof backPath !== 'string')
    return false
  const resolved = router.resolve(backPath)
  if (resolved.name !== 'admin')
    return false
  const targetSection = typeof resolved.query.section === 'string' && resolved.query.section
    ? resolved.query.section
    : 'nodes'
  const targetNode = typeof resolved.query.node === 'string'
    ? resolved.query.node.trim()
    : ''
  return targetSection === 'nodes' && !targetNode
}

function choose(id: string, event?: MouseEvent): void {
  if (busy.value)
    return
  if (id === selected.value?.id && selection.value.mode === 'selected')
    return
  if (event?.currentTarget instanceof HTMLElement)
    event.currentTarget.focus()
  if (!hasUnsavedChanges.value)
    pageStatusNotice.value = ''
  const fromMobileList = !isDesktop.value && selection.value.mode === 'list'
  if (fromMobileList) {
    const scrollY = typeof window !== 'undefined'
      ? window.scrollY
      : null
    savedListScrollY.value = scrollY
    lastInteractedNodeId.value = id
    lastSavedNodeSummary.value = null
    adminNodesViewState.value = {
      ...adminNodesViewState.value,
      listScrollY: scrollY,
      lastNodeId: id,
    }
  }
  else if (nodeListEl.value) {
    adminNodesViewState.value = {
      ...adminNodesViewState.value,
      listScrollTop: nodeListEl.value.scrollTop,
    }
  }
  const location = {
    query: {
      ...route.query,
      section: 'nodes',
      node: id,
    },
  }
  if (fromMobileList)
    void router.push(location)
  else
    void router.replace(location)
}

function requestBackToList(event?: MouseEvent): void {
  if (busy.value)
    return
  const triggerEl = event?.currentTarget instanceof HTMLElement
    ? event.currentTarget
    : null
  if (triggerEl)
    triggerEl.focus()
  // Resize edge case: desktop default node (/admin?section=nodes without ?node) narrowed to mobile.
  if (!queryNodeId.value && pinnedNodeId.value) {
    draftGuard.requestSwitch(
      '__list__',
      '节点列表',
      () => {
        pinnedNodeId.value = null
        loadDraftFromNode(undefined)
      },
      triggerEl,
    )
    return
  }
  // Prefer popping the known same-document list history entry instead of creating duplicate adjacent list entries.
  if (!isDesktop.value && queryNodeId.value && typeof window !== 'undefined' && isNodesListHistoryEntry(window.history.state?.back)) {
    router.back()
    return
  }
  void clearNodeQuery()
}

function dismissRemovedNode(event?: MouseEvent): void {
  if (busy.value)
    return
  const triggerEl = event?.currentTarget instanceof HTMLElement
    ? event.currentTarget
    : null
  if (triggerEl)
    triggerEl.focus()
  const targetLabel = isDesktop.value
    ? '当前可用节点'
    : '节点列表'
  draftGuard.requestSwitch(
    '__dismiss_removed__',
    targetLabel,
    () => {
      pinnedNodeId.value = null
      removedNodeLabel.value = null
      loadDraftFromNode(undefined)
      if (queryNodeId.value)
        void clearNodeQuery()
    },
    triggerEl,
  )
}

async function openEnrollFromEditor(): Promise<void> {
  enrollExpanded.value = true
  await nextTick()
  if (typeof document !== 'undefined')
    document.getElementById('admin-node-enroll-panel')?.scrollIntoView({ behavior: 'auto', block: 'nearest' })
}

function viewMonitor(): void {
  if (!draft.value || selected.value?.hidden)
    return
  adminNodesViewState.value = {
    ...adminNodesViewState.value,
    searchQuery: searchQuery.value,
    statusFilter: statusFilter.value,
    lastNodeId: draft.value.id,
    listScrollTop: nodeListEl.value?.scrollTop ?? adminNodesViewState.value.listScrollTop,
    listScrollY: isDesktop.value
      ? adminNodesViewState.value.listScrollY
      : (savedListScrollY.value ?? adminNodesViewState.value.listScrollY),
  }
  void router.push(`/instance/${draft.value.id}`)
}

function openConfirm(action: NodeDangerActionKind): void {
  if (!selected.value || isDangerActionDisabled(action))
    return
  if (dangerOutcome.value === 'failed' || dangerOutcome.value === 'saved')
    dangerAction.clearFeedback()
  pageStatusNotice.value = ''
  confirmTarget.value = {
    action,
    nodeId: selected.value.id,
    nodeName: selected.value.name,
  }
}

function closeConfirm(): void {
  if (busy.value)
    return
  confirmTarget.value = null
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

async function finishDeletedNodeNavigation(deletedId: string, bannerMessage?: string): Promise<void> {
  if (bannerMessage)
    pageStatusNotice.value = bannerMessage
  if (pinnedNodeId.value === deletedId)
    pinnedNodeId.value = null
  if (removedNodeLabel.value?.id === deletedId)
    removedNodeLabel.value = null
  if (loadedNodeId.value === deletedId)
    loadDraftFromNode(undefined)
  if (lastSavedNodeSummary.value?.nodeId === deletedId)
    lastSavedNodeSummary.value = null
  if (lastInteractedNodeId.value === deletedId) {
    lastInteractedNodeId.value = null
    adminNodesViewState.value = {
      ...adminNodesViewState.value,
      lastNodeId: null,
    }
  }
  emit('nodeDeleted', deletedId)
  if (queryNodeId.value === deletedId)
    await clearNodeQuery()
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
  pageStatusNotice.value = ''
  clearPendingSave(node.id)
  const result = await saveAction.run(async () => {
    await getSharedApi().post(`admin/nodes/${encodeURIComponent(node.id)}`, node)
  }, {
    message: '已保存',
    onCommitted: () => {
      lastSavedNodeSummary.value = {
        nodeId: node.id,
        nodeName: node.name,
      }
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
  issuedSecret.value = null
  const result = await enrollAction.run(async () => {
    const response = await getSharedApi().post<{ token: string }>('admin/enrollment', { ttl_seconds: ttlHours.value * 3600 })
    if (isSameActiveSession(sessionGen, auth.loggedIn)) {
      issuedSecret.value = {
        token: response.token,
        kind: 'enroll',
      }
    }
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
  return `节点「${nodeName}」的${NODE_DANGER_ACTIONS[action].label}结果未确认（${detail}）。请先重新同步核对当前状态，勿直接重复提交。`
}

async function execute(): Promise<void> {
  if (!confirmTarget.value || activeConfirmUncertain.value || confirmTargetInvalidReason.value)
    return
  const sessionGen = getSessionGeneration()
  const { action, nodeId: id, nodeName } = confirmTarget.value
  const key = `${action}:${id}`
  issuedSecret.value = null
  pageStatusNotice.value = ''
  let committedDelete = false
  const result = await dangerAction.run(async () => {
    if (action === 'rotate') {
      const response = await getSharedApi().post<{ agent_token: string }>(`admin/nodes/${encodeURIComponent(id)}/rotate`)
      if (isSameActiveSession(sessionGen, auth.loggedIn)) {
        issuedSecret.value = {
          token: response.agent_token,
          kind: 'rotate',
          nodeId: id,
          nodeName,
        }
      }
    }
    else {
      await getSharedApi().post(`admin/nodes/${encodeURIComponent(id)}/${action}`)
    }
  }, {
    message: action === 'delete'
      ? `已删除节点「${nodeName}」`
      : '操作已完成',
    riskyWrite: true,
    // Backend delete_node commits deletion before optional metrics cleanup, which can return structured 503.
    isUncertainError: action === 'delete'
      ? cause => cause instanceof ApiError && cause.statusCode === 503
      : undefined,
    uncertainMessage: detail => buildDangerUncertainMessage(action, nodeName, detail),
    onCommitted: () => {
      if (!isSameActiveSession(sessionGen, auth.loggedIn))
        return
      confirmTarget.value = null
      if (pendingUncertainDangers.value[key]) {
        const next = { ...pendingUncertainDangers.value }
        delete next[key]
        pendingUncertainDangers.value = next
      }
      if (action === 'delete')
        committedDelete = true
    },
  })
  if (committedDelete && isSameActiveSession(sessionGen, auth.loggedIn)) {
    await finishDeletedNodeNavigation(
      id,
      result === 'saved'
        ? `已删除节点「${nodeName}」`
        : undefined,
    )
    return
  }
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
  let verifiedDeleted = false
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
          if (confirmTarget.value?.action === 'delete' && confirmTarget.value.nodeId === entry.nodeId)
            confirmTarget.value = null
          verifiedDeleted = true
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
          if (confirmTarget.value?.action === 'revoke' && confirmTarget.value.nodeId === entry.nodeId)
            confirmTarget.value = null
          return {
            resolved: true,
            message: `已核实：节点「${entry.nodeName}」已不存在`,
          }
        }
        if (freshNode.disabled) {
          if (confirmTarget.value?.action === 'revoke' && confirmTarget.value.nodeId === entry.nodeId)
            confirmTarget.value = null
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
  if (verifiedDeleted && isSameActiveSession(sessionGen, auth.loggedIn)) {
    await finishDeletedNodeNavigation(entry.nodeId, `已核实：节点「${entry.nodeName}」已删除`)
    return
  }
  if (!ok && dangerResyncError.value && isSameActiveSession(sessionGen, auth.loggedIn) && pendingUncertainDangers.value[entry.key])
    pendingUncertainDangers.value[entry.key]!.resyncError = dangerResyncError.value
}
</script>

<template>
  <div class="space-y-4">
    <div v-if="showTopDangerRefreshAlert" class="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/40 bg-card p-3">
      <p role="alert" class="text-sm text-destructive">
        {{ dangerError }}
      </p>
      <Button v-if="dangerNeedsResync" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncDanger()">
        重新同步
      </Button>
    </div>
    <p v-if="topStatusMessage" role="status" class="rounded-lg border border-border bg-card px-3 py-2 text-sm text-success">
      {{ topStatusMessage }}
    </p>

    <section v-if="showNodeList || enrollExpanded" class="rounded-xl border bg-card text-card-foreground shadow-sm">
      <div class="flex flex-wrap items-center justify-between gap-2 px-4 py-3">
        <div class="space-y-0.5">
          <h2 class="text-base font-semibold">
            接入节点
          </h2>
          <p class="text-xs text-muted-foreground">
            生成一次性注册令牌，仅保留在当前页面内存中。
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          class="min-h-11"
          :aria-expanded="enrollExpanded"
          aria-controls="admin-node-enroll-panel"
          @click="enrollExpanded = !enrollExpanded"
        >
          <Icon :icon="enrollExpanded ? 'lucide:chevron-up' : 'lucide:chevron-down'" class="size-4" aria-hidden="true" />
          {{ enrollExpanded ? '收起接入节点' : '展开接入节点' }}
        </Button>
      </div>
      <div v-show="enrollExpanded" id="admin-node-enroll-panel" class="space-y-4 border-t px-4 py-4">
        <form class="flex flex-wrap items-end gap-3" @submit.prevent="enroll">
          <label class="grid gap-2 text-sm">
            注册令牌有效期（小时）
            <Input v-model="ttlHours" type="number" min="1" max="24" required class="h-11 max-w-48" :disabled="busy" />
          </label>
          <Button type="submit" class="min-h-11" :disabled="busy || Boolean(enrollUncertain)">
            {{ enrollBusy ? '正在生成…' : '生成一次性注册令牌' }}
          </Button>
        </form>
        <p class="text-sm text-muted-foreground">
          令牌仅用于注册一个独立新节点，请通过受保护的配置文件交给对应服务器。
        </p>
        <div v-if="enrollUncertain" class="flex flex-wrap items-center gap-2">
          <p role="alert" class="text-sm text-destructive">
            {{ enrollUncertain }}
          </p>
          <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="acknowledgeEnrollUncertain">
            已知悉，允许重新生成
          </Button>
        </div>
        <p v-else-if="enrollError" role="alert" class="text-sm text-destructive">
          {{ enrollError }}
        </p>
        <p v-if="enrollSuccess" role="status" class="text-sm text-success">
          {{ enrollSuccess }}
        </p>
      </div>
    </section>

    <div v-if="issuedSecret" class="space-y-2 rounded-xl border bg-card p-4 text-card-foreground shadow-sm">
      <div class="space-y-0.5">
        <label for="agent-secret" class="text-sm font-medium">新令牌（离开页面后清除）</label>
        <p class="text-xs text-muted-foreground">
          {{ issuedSecretAttribution }}
        </p>
      </div>
      <Input id="agent-secret" :model-value="issuedSecret.token" readonly autocomplete="off" class="h-11 font-mono" @focus="($event.target as HTMLInputElement).select()" />
      <Button variant="outline" type="button" class="min-h-11" @click="issuedSecret = null">
        隐藏令牌
      </Button>
    </div>

    <div :class="isDesktop ? 'grid items-start gap-4 lg:grid-cols-[280px_minmax(0,1fr)]' : 'space-y-4'">
      <CardX v-if="showNodeList" title="节点列表" content-class="space-y-3">
        <div class="space-y-2">
          <div class="grid gap-1.5 text-sm">
            <label for="admin-nodes-search" class="font-medium">
              搜索节点
            </label>
            <div class="flex items-center gap-2">
              <Input
                id="admin-nodes-search"
                :ref="setSearchInputRef"
                v-model="searchQuery"
                type="search"
                class="h-11"
                placeholder="按名称、分组、地区、备注或 ID 前缀"
                autocomplete="off"
              />
              <Button
                v-if="hasActiveFilter"
                type="button"
                variant="outline"
                class="min-h-11"
                @click="clearFiltersAndFocusSearch"
              >
                清空
              </Button>
            </div>
          </div>
          <div role="group" aria-label="按节点状态筛选" class="flex flex-wrap gap-1.5">
            <Button
              v-for="option in NODE_LIST_FILTER_OPTIONS"
              :key="option.value"
              type="button"
              :variant="statusFilter === option.value ? 'default' : 'outline'"
              class="min-h-11"
              :aria-pressed="statusFilter === option.value"
              @click="statusFilter = option.value"
            >
              {{ option.label }}
            </Button>
          </div>
          <p class="text-xs text-muted-foreground">
            共 {{ nodes.length }} 台节点，当前显示 {{ filteredNodes.length }} 台
          </p>
        </div>

        <div v-if="isSelectedFilteredOut && selected" role="status" class="space-y-2 rounded-md border border-border bg-muted/40 p-3 text-xs text-muted-foreground">
          <p>
            当前正在编辑的节点「{{ selected.name }}」不在当前筛选结果中；筛选不会自动切换编辑对象。
          </p>
          <Button type="button" variant="outline" class="min-h-11 w-full" @click="clearFiltersAndFocusSearch">
            清除筛选并显示当前节点
          </Button>
        </div>

        <div v-if="!nodes.length" class="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
          尚无节点。展开上方“接入节点”生成注册令牌并安装 Agent 后，节点会出现在这里。
        </div>
        <div v-else-if="!filteredNodes.length" class="space-y-2 rounded-md border border-dashed p-4 text-sm text-muted-foreground">
          <p>
            没有符合当前搜索或筛选条件的节点。
          </p>
          <Button type="button" variant="outline" class="min-h-11" @click="clearFiltersAndFocusSearch">
            重置筛选
          </Button>
        </div>
        <ul v-else ref="nodeListEl" aria-label="可管理节点" class="space-y-1.5" :class="isDesktop ? 'max-h-[70vh] overflow-y-auto pr-1' : ''">
          <li v-for="node in filteredNodes" :key="node.id">
            <button
              :ref="el => setNodeButtonRef(node.id, el)"
              type="button"
              :disabled="busy"
              :aria-current="selected?.id === node.id ? 'true' : undefined"
              :data-last-active="!selected && lastInteractedNodeId === node.id ? 'true' : undefined"
              class="flex min-h-11 w-full flex-col items-start gap-1 rounded-lg border px-3 py-2.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
              :class="isNodeHighlighted(node.id) ? 'border-primary bg-primary/10 text-foreground' : 'border-border bg-background/60 hover:bg-accent'"
              @click="choose(node.id, $event)"
            >
              <div class="flex w-full items-center justify-between gap-2">
                <span class="truncate text-sm font-medium">{{ node.name }}</span>
                <span class="shrink-0 font-mono text-xs text-muted-foreground">{{ node.id.slice(0, 8) }}</span>
              </div>
              <div class="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                <span v-if="node.group">{{ node.group }}</span>
                <span v-if="node.region">{{ node.region }}</span>
                <Badge v-if="node.hidden" variant="secondary">
                  已隐藏
                </Badge>
                <Badge v-if="node.disabled" variant="destructive">
                  凭据已撤销
                </Badge>
                <Badge v-if="node.traffic_source === 'provider'" variant="outline">
                  服务商流量
                </Badge>
                <Badge v-if="selected?.id === node.id && hasUnsavedChanges" variant="default">
                  未保存
                </Badge>
                <Badge v-else-if="!selected && lastInteractedNodeId === node.id" variant="outline">
                  上次查看
                </Badge>
              </div>
            </button>
          </li>
        </ul>
      </CardX>

      <div v-if="showEditorPane" class="space-y-4">
        <CardX v-if="disappearedNode" title="节点已被移除" content-class="space-y-4">
          <p role="alert" class="text-sm text-destructive">
            节点「{{ disappearedNode.name }}」（ID：{{ disappearedNode.id }}）在最新同步中已不存在，无法继续保存。为避免误改其他节点，已暂停自动切换编辑对象{{ hasUnsavedChanges ? '；下方已以只读方式完整保留您的未保存草稿供复制。' : '。' }}
          </p>
          <p v-if="saveError" role="alert" class="text-sm text-destructive">
            {{ saveError }}
          </p>
          <div v-if="hasUnsavedChanges && removedDraftBackup" class="grid gap-1.5 text-sm">
            <label for="removed-node-draft-backup" class="font-medium">
              未保存的节点草稿快照（只读，可复制）
            </label>
            <textarea
              id="removed-node-draft-backup"
              :value="removedDraftBackup"
              readonly
              rows="10"
              class="rounded-md border border-input bg-muted/20 px-3 py-2 font-mono text-xs"
            />
          </div>
          <div class="flex flex-wrap gap-2">
            <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="dismissRemovedNode">
              {{ isDesktop ? '关闭提示并查看当前节点' : '返回节点列表' }}
            </Button>
          </div>
        </CardX>

        <CardX v-else-if="selection.mode === 'missing'" title="未找到该节点" content-class="space-y-4">
          <p role="alert" class="text-sm text-destructive">
            未找到节点 ID「{{ missingRequestedId }}」，可能已被删除或链接无效。为避免误改其他节点，已暂停加载编辑表单。
          </p>
          <div class="flex flex-wrap gap-2">
            <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="requestBackToList">
              返回节点列表
            </Button>
          </div>
        </CardX>

        <CardX v-else-if="selection.mode === 'empty'" title="节点配置" content-class="space-y-2">
          <p class="text-sm text-muted-foreground">
            尚无已注册节点。请先在上方生成一次性注册令牌并安装 Agent。
          </p>
        </CardX>

        <CardX v-else-if="selection.mode === 'selected' && draft && selected" title="节点配置" content-class="space-y-5">
          <div class="flex flex-wrap items-center justify-between gap-3 border-b pb-4">
            <div class="space-y-1">
              <div v-if="!isDesktop" class="pb-1">
                <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="requestBackToList">
                  <Icon icon="lucide:arrow-left" class="size-4" aria-hidden="true" />
                  返回节点列表
                </Button>
              </div>
              <div class="flex flex-wrap items-center gap-2">
                <h2 ref="editorHeadingRef" tabindex="-1" class="text-lg font-semibold outline-none">
                  {{ selected.name }}
                </h2>
                <span class="font-mono text-xs text-muted-foreground">{{ selected.id.slice(0, 8) }}</span>
                <Badge v-if="selected.hidden" variant="secondary">
                  已隐藏
                </Badge>
                <Badge v-if="selected.disabled" variant="destructive">
                  凭据已撤销
                </Badge>
                <Badge v-if="selected.traffic_source === 'provider'" variant="outline">
                  服务商流量
                </Badge>
              </div>
              <p v-if="hasUnsavedChanges" role="status" class="text-sm text-muted-foreground">
                有未保存修改；后台状态刷新不会覆盖未保存的节点输入。
              </p>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <Button variant="outline" type="button" class="min-h-11" :disabled="selected.hidden" @click="viewMonitor">
                查看监控
              </Button>
            </div>
          </div>

          <form id="node-edit-form" class="space-y-5" :aria-busy="busy" @submit.prevent="save">
            <fieldset :disabled="busy" class="grid gap-4 rounded-lg border p-4 sm:grid-cols-2 lg:grid-cols-3">
              <legend class="px-1 font-medium">
                显示与分组
              </legend>
              <label class="grid gap-2 text-sm">名称<Input v-model="draft.name" class="h-11" required maxlength="128" /></label>
              <label class="grid gap-2 text-sm">地区代码<Input v-model="draft.region" class="h-11" placeholder="例如 SG" maxlength="16" /></label>
              <label class="grid gap-2 text-sm">分组<Input v-model="draft.group" class="h-11" maxlength="128" /></label>
              <label class="grid gap-2 text-sm">排序权重<Input v-model="draft.weight" class="h-11" type="number" min="-2147483648" max="2147483647" step="1" required /></label>
              <label class="flex min-h-11 items-center gap-2 text-sm sm:col-span-2"><input v-model="draft.hidden" type="checkbox" class="size-4 accent-primary">从监控视图隐藏（管理列表保留）</label>
              <div class="grid gap-2 text-sm sm:col-span-2 lg:col-span-3">
                <label for="node-tags">小面板标签</label>
                <Input id="node-tags" v-model="draft.tags" class="h-11" placeholder="马来西亚<gray>;V4<green>;V6<green>;带宽1G<violet>" maxlength="512" aria-describedby="node-tags-help" />
                <p id="node-tags-help" class="text-muted-foreground">
                  可添加地区、线路、协议和带宽等信息；用分号分隔，颜色格式为 标签文字&lt;颜色英文名&gt;。也可以用下方按钮添加。
                </p>
                <div class="flex flex-wrap items-end gap-2">
                  <label class="grid min-w-36 flex-1 gap-1">标签文字<Input v-model="newTagText" class="h-11" maxlength="40" placeholder="例如：带宽1G" @keydown.enter.prevent="addTag" /></label>
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
              <label class="grid gap-2 text-sm sm:col-span-2 lg:col-span-3">公开备注<textarea v-model="draft.public_remark" rows="3" maxlength="2048" class="rounded-md border border-input bg-background px-3 py-2 focus-visible:ring-2 focus-visible:ring-ring" /></label>
            </fieldset>

            <fieldset :disabled="busy" class="grid gap-4 rounded-lg border p-4 sm:grid-cols-2 lg:grid-cols-3">
              <legend class="px-1 font-medium">
                网络与互测
              </legend>
              <label class="grid gap-2 text-sm">公网 IPv4（可选）<Input v-model="draft.ipv4" class="h-11" maxlength="15" aria-describedby="node-ip-privacy" /></label>
              <label class="grid gap-2 text-sm">公网 IPv6（可选）<Input v-model="draft.ipv6" class="h-11" maxlength="45" aria-describedby="node-ip-privacy" /></label>
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
            </fieldset>

            <fieldset :disabled="busy" class="grid gap-4 rounded-lg border p-4 sm:grid-cols-2 lg:grid-cols-3">
              <legend class="px-1 font-medium">
                费用与到期
              </legend>
              <label class="grid gap-2 text-sm">每周期价格<Input v-model="draft.price" class="h-11" type="number" min="0" max="1000000000" step="0.01" required /></label>
              <label class="grid gap-2 text-sm">币种（大写三字母）<Input v-model="draft.currency" class="h-11" placeholder="USD" pattern="[A-Z]{3}" minlength="3" maxlength="3" required /></label>
              <label class="grid gap-2 text-sm">计费周期（天）<Input v-model="draft.billing_cycle_days" class="h-11" type="number" min="0" max="36500" step="1" required /></label>
              <label class="grid gap-2 text-sm">到期时间（本地时间）<Input v-model="expires" class="h-11" type="datetime-local" /></label>
              <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="draft.auto_renewal" type="checkbox" class="size-4 accent-primary">自动续费标记</label>
              <p class="text-sm text-muted-foreground sm:col-span-2 lg:col-span-3">
                这些信息用于资产记录和提醒，Senlun 不会执行支付或扣款。
              </p>
            </fieldset>

            <fieldset :disabled="busy" class="grid gap-4 rounded-lg border p-4 sm:grid-cols-3">
              <legend class="px-1 font-medium">
                流量周期
              </legend>
              <label class="grid gap-2 text-sm sm:col-span-3">卡片、列表及额度告警的数据来源
                <SelectControl aria-label="流量数据来源" :model-value="draft.traffic_source ?? 'local'" :disabled="busy" :options="[{ value: 'local', label: '本地网卡周期流量' }, { value: 'provider', label: '服务商记录流量' }]" @update:model-value="draft.traffic_source = $event" />
              </label>
              <p class="text-sm text-muted-foreground sm:col-span-3">
                保存后同时应用于此节点的流量显示与额度告警。服务商未配置、过期或失败时不会切回本地；下列额度、计数方式及重置日只用于本地统计。切换会结束旧来源的阈值评估，不发送恢复通知。
              </p>
              <p role="status" class="text-sm text-muted-foreground sm:col-span-3">
                当前流量状态：{{ trafficSelectionDetails(selected.selected_traffic) }}
              </p>
              <label class="grid gap-2 text-sm">每周期额度（GiB；0 为不限）<Input v-model="quotaGiB" class="h-11" type="number" min="0" :max="Number.MAX_SAFE_INTEGER / 1024 ** 3" step="any" required /></label>
              <label class="grid gap-2 text-sm">流量计数方式<SelectControl v-model="draft.traffic_limit_type" aria-label="流量计数方式" :disabled="busy" :options="[{ value: 'sum', label: '上传 + 下载' }, { value: 'max', label: '上传、下载中较大值' }, { value: 'min', label: '上传、下载中较小值' }, { value: 'up', label: '仅上传' }, { value: 'down', label: '仅下载' }]" /></label>
              <label class="grid gap-2 text-sm">每月重置日（1–31，UTC）<Input v-model="draft.traffic_reset_day" class="h-11" type="number" min="1" max="31" step="1" required /></label>
              <p class="text-sm text-muted-foreground sm:col-span-3">
                按 UTC 00:00 重置；当月没有所选日期时，在该月最后一天重置。新日期立即生效，现有计数保留至下一个新日期。流量是近似统计，账单以服务商为准。
              </p>
              <p v-if="selected.traffic_next_reset_unix_ms != null" class="text-sm text-muted-foreground sm:col-span-3">
                下次重置：{{ utcTime(selected.traffic_next_reset_unix_ms) }}
              </p>
            </fieldset>
          </form>

          <div class="space-y-2 rounded-lg border border-border/80 bg-muted/20 p-4">
            <h3 class="text-sm font-medium">
              本周期本地流量校正
            </h3>
            <p class="text-sm text-muted-foreground">
              仅调整当前本地统计周期的累计值，独立提交生效，不会覆盖上方尚未保存的节点配置修改。
            </p>
            <AdminTrafficCorrection :node="selected" :disabled="busy" :refresh="props.refresh" />
          </div>

          <div class="space-y-2 border-t pt-4">
            <div class="flex flex-wrap items-center gap-2">
              <Button form="node-edit-form" class="min-h-11" type="submit" :disabled="busy">
                {{ saveBusy ? '正在处理…' : '保存节点' }}
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
            <p v-if="selected.hidden" class="text-sm text-muted-foreground">
              此节点已从监控页面和探测历史中隐藏。取消隐藏并保存后可查看。
            </p>
          </div>

          <section aria-label="危险操作" class="space-y-3 rounded-lg border border-destructive/30 p-4">
            <div class="space-y-1">
              <h3 class="text-sm font-semibold text-destructive">
                危险操作
              </h3>
              <p class="text-xs text-muted-foreground">
                以下操作会立即影响节点「{{ selected.name }}」的凭据、周期统计或节点数据，执行前需二次确认。
              </p>
              <div v-if="selected.disabled" class="space-y-2 text-xs text-muted-foreground">
                <p>
                  {{ revokedGuidanceText }}
                </p>
                <Button v-if="!isDesktop && !enrollExpanded" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="openEnrollFromEditor">
                  打开接入节点面板
                </Button>
              </div>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <Button
                v-for="item in dangerActionsList"
                :key="item.action"
                variant="outline"
                type="button"
                class="min-h-11"
                :disabled="isDangerActionDisabled(item.action)"
                :class="item.action === 'delete' ? 'border-destructive/40 text-destructive hover:bg-destructive/10' : ''"
                @click="openConfirm(item.action)"
              >
                {{ item.label }}
              </Button>
            </div>
            <div v-if="confirmTarget === null && selectedNodePendingDangers.length" class="space-y-2">
              <div v-for="entry in selectedNodePendingDangers" :key="entry.key" class="flex flex-wrap items-center gap-2 rounded-md border border-destructive/40 p-3">
                <p role="alert" class="text-sm text-destructive">
                  {{ formatPendingAlert(entry.message, entry.resyncError) }}
                </p>
                <Button v-if="entry.verifiable" type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncDanger(entry)">
                  重新同步
                </Button>
                <template v-else>
                  <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="resyncDanger(entry)">
                    刷新管理状态
                  </Button>
                  <Button type="button" variant="outline" class="min-h-11" :disabled="busy" @click="unlockUncertainDanger(entry.key)">
                    我已核对状态，解除锁定
                  </Button>
                </template>
              </div>
            </div>
            <p v-if="confirmTarget === null && !pageStatusNotice && dangerSuccess" role="status" class="text-sm text-success">
              {{ dangerSuccess }}
            </p>
          </section>
        </CardX>
      </div>
    </div>

    <Dialog :open="draftGuardOpen" @update:open="value => { if (!value) cancelDraftGuard() }">
      <DialogContent @close-auto-focus="restoreDraftGuardFocus">
        <DialogHeader>
          <DialogTitle>放弃未保存修改？</DialogTitle>
          <DialogDescription v-if="draftGuardSwitchTarget">
            切换到 {{ draftGuardSwitchTarget }} 会丢弃当前节点未保存的修改，不会保存任何更改。
          </DialogDescription>
          <DialogDescription v-else-if="draftGuardPending?.kind === 'refresh'">
            刷新管理数据会丢弃当前节点未保存的修改，不会保存任何更改。
          </DialogDescription>
          <DialogDescription v-else>
            离开当前节点编辑会丢弃未保存的修改，不会保存任何更改。
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

    <Dialog :open="confirmTarget !== null" @update:open="value => { if (!value) closeConfirm() }">
      <DialogContent :show-close="!busy" @escape-key-down="event => { if (busy) event.preventDefault() }" @interact-outside="event => { if (busy) event.preventDefault() }">
        <DialogHeader>
          <DialogTitle>{{ activeConfirmMeta?.label ?? '' }}</DialogTitle>
          <DialogDescription>{{ activeConfirmDescription }}</DialogDescription>
        </DialogHeader>
        <p v-if="confirmTargetInvalidReason" role="alert" class="text-sm text-destructive">
          {{ confirmTargetInvalidReason }}
        </p>
        <p v-else-if="activeConfirmUncertain" role="alert" class="text-sm text-destructive">
          {{ formatPendingAlert(activeConfirmUncertain.message, activeConfirmUncertain.resyncError) }}
        </p>
        <p v-else-if="dangerError" role="alert" class="text-sm text-destructive">
          {{ dangerError }}
        </p>
        <p v-if="!confirmTargetInvalidReason && !activeConfirmUncertain && dangerSuccess" role="status" class="text-sm text-success">
          {{ dangerSuccess }}
        </p>
        <DialogFooter>
          <Button variant="outline" class="min-h-11" :disabled="busy" @click="closeConfirm">
            取消
          </Button>
          <template v-if="activeConfirmUncertain && !confirmTargetInvalidReason">
            <Button v-if="activeConfirmUncertain.verifiable" variant="outline" class="min-h-11" :disabled="busy" @click="resyncDanger(activeConfirmUncertain)">
              重新同步
            </Button>
            <template v-else>
              <Button variant="outline" class="min-h-11" :disabled="busy" @click="resyncDanger(activeConfirmUncertain)">
                刷新管理状态
              </Button>
              <Button variant="outline" class="min-h-11" :disabled="busy" @click="unlockUncertainDanger(activeConfirmUncertain.key)">
                我已核对状态，解除锁定
              </Button>
            </template>
          </template>
          <Button variant="destructive" class="min-h-11" :disabled="busy || Boolean(activeConfirmUncertain) || Boolean(confirmTargetInvalidReason)" @click="execute">
            {{ confirmButtonLabel }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
