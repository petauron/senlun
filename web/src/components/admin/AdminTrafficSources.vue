<script setup lang="ts">
import type { RouteLocationNormalized } from 'vue-router'
import type { ExternalTrafficConfig, ExternalTrafficSource, ManagedNode } from '@/utils/admin'
import { computed, inject, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { onBeforeRouteLeave, onBeforeRouteUpdate } from 'vue-router'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import SelectControl from '@/components/ui/SelectControl.vue'
import { useAdminAction } from '@/composables/useAdminAction'
import { ADMIN_DRAFT_REGISTRY_KEY } from '@/composables/useAdminDraftGuard'
import { useAuthStore } from '@/stores/auth'
import { createSerializedRefresh } from '@/utils/adminRefresh'
import { getSharedApi } from '@/utils/api'
import { decideTrafficDeparture, shouldWarnBeforeUnload, unmountLeaveResult } from '@/utils/authFlow'

interface PendingUncertainTrafficSave {
  nodeId: string
  nodeName: string
  config: ExternalTrafficConfig
  hadCredentialBefore: boolean
  includedCredential: boolean
  message: string
  resyncError: string
}

interface PendingUncertainTrafficRemove {
  nodeId: string
  nodeName: string
  message: string
  resyncError: string
}

const props = defineProps<{ nodes: ManagedNode[] }>()
const auth = useAuthStore()
const draftRegistry = inject(ADMIN_DRAFT_REGISTRY_KEY, null)
const selectedId = ref(props.nodes[0]?.id ?? '')
const sources = ref<ExternalTrafficSource[]>([])
const keyReady = ref(false)
const loading = ref(false)
const loadError = ref('')
const username = ref('')
const password = ref('')
const bearerToken = ref('')
const confirmRemove = ref(false)
const pendingNodeId = ref<string | null>(null)
const pendingLeave = ref(false)
const pendingUncertainSaves = ref<Record<string, PendingUncertainTrafficSave>>({})
const pendingUncertainRemovals = ref<Record<string, PendingUncertainTrafficRemove>>({})
let resolveLeave: ((leave: boolean) => void) | null = null
let discardFocusTarget: HTMLElement | null = null
let formLoaded = false
let beforeUnloadBound = false
let disposed = false

function emptyConfig(): ExternalTrafficConfig {
  return {
    name: '',
    enabled: false,
    auth_mode: 'none',
    traffic_url: '',
    login_url: '',
    refresh_url: '',
    username_field: 'email',
    password_field: 'password',
    refresh_token_field: 'refresh_token',
    access_pointer: '/data/access_token',
    refresh_pointer: '/data/refresh_token',
    up_pointer: '',
    down_pointer: '',
    used_pointer: '',
    limit_pointer: '',
    limit_unit: 'bytes',
    reset_pointer: '',
  }
}

function sameTrafficConfig(a: ExternalTrafficConfig, b: ExternalTrafficConfig): boolean {
  return (Object.keys(a) as (keyof ExternalTrafficConfig)[])
    .every(key => a[key] === b[key])
}

const draft = ref<ExternalTrafficConfig>(emptyConfig())
const savedDraft = ref<ExternalTrafficConfig>(emptyConfig())
const current = computed(() => sources.value.find(source => source.node_id === selectedId.value))
const currentNodeName = computed(() => props.nodes.find(node => node.id === selectedId.value)?.name ?? selectedId.value)
const canReuseCredential = computed(() => current.value?.has_credential && current.value.config.auth_mode === draft.value.auth_mode)
const hasUnsavedChanges = computed(() => Boolean(username.value || password.value || bearerToken.value)
  || !sameTrafficConfig(draft.value, savedDraft.value))
const pendingNodeName = computed(() => props.nodes.find(node => node.id === pendingNodeId.value)?.name)

const sourcesRefresher = createSerializedRefresh<void, boolean>(
  async (rethrow, isLatest) => {
    if (disposed)
      return
    loadError.value = ''
    try {
      const result = await getSharedApi().get<{ sources: ExternalTrafficSource[], credential_key_ready: boolean }>('admin/traffic-sources')
      if (!disposed && isLatest()) {
        sources.value = result.sources
        keyReady.value = result.credential_key_ready
        if (!formLoaded) {
          selectSource()
          formLoaded = true
        }
      }
    }
    catch (cause) {
      if (!disposed && isLatest()) {
        loadError.value = cause instanceof Error
          ? cause.message
          : String(cause)
      }
      if (rethrow)
        throw cause
    }
  },
  (nextLoading) => {
    loading.value = nextLoading
  },
)

async function load(rethrow = false): Promise<void> {
  await sourcesRefresher.refresh(rethrow)
}

const saveAction = useAdminAction(() => load(true))
const removeAction = useAdminAction(() => load(true))
const {
  busy: saveBusy,
  error: saveError,
  resyncError: saveResyncError,
  success: saveSuccess,
  outcome: saveOutcome,
  needsResync: saveNeedsResync,
} = saveAction
const {
  busy: removeBusy,
  error: removeError,
  resyncError: removeResyncError,
  success: removeSuccess,
  outcome: removeOutcome,
  needsResync: removeNeedsResync,
} = removeAction

const busy = computed(() => saveBusy.value || removeBusy.value)
const currentPendingSave = computed(() => {
  if (!selectedId.value)
    return null
  return pendingUncertainSaves.value[selectedId.value] ?? null
})
const currentPendingRemove = computed(() => {
  if (!selectedId.value)
    return null
  return pendingUncertainRemovals.value[selectedId.value] ?? null
})

function formatPendingAlert(message: string, resyncFailure: string): string {
  return resyncFailure
    ? `${message}（重新同步失败：${resyncFailure}）`
    : message
}

const pageAlert = computed(() => {
  if (currentPendingSave.value)
    return formatPendingAlert(currentPendingSave.value.message, currentPendingSave.value.resyncError)
  if (!confirmRemove.value && currentPendingRemove.value)
    return formatPendingAlert(currentPendingRemove.value.message, currentPendingRemove.value.resyncError)
  if (saveError.value)
    return saveError.value
  if (!confirmRemove.value && removeOutcome.value === 'refresh_failed' && removeError.value)
    return removeError.value
  return loadError.value
})

const pageSuccess = computed(() => {
  if (saveSuccess.value)
    return saveSuccess.value
  if (!confirmRemove.value && removeSuccess.value)
    return removeSuccess.value
  return ''
})
const pageNeedsResync = computed(() => {
  if (saveNeedsResync.value || Boolean(currentPendingSave.value))
    return true
  if (confirmRemove.value)
    return false
  return removeNeedsResync.value || Boolean(currentPendingRemove.value)
})

function handleBeforeUnload(event: BeforeUnloadEvent): void {
  if (!shouldWarnBeforeUnload(auth.loggedIn, hasUnsavedChanges.value))
    return
  event.preventDefault()
  event.returnValue = ''
}

function syncBeforeUnload(active: boolean): void {
  if (typeof window === 'undefined')
    return
  if (active && !beforeUnloadBound) {
    window.addEventListener('beforeunload', handleBeforeUnload)
    beforeUnloadBound = true
  }
  else if (!active && beforeUnloadBound) {
    window.removeEventListener('beforeunload', handleBeforeUnload)
    beforeUnloadBound = false
  }
}

function selectSource(): void {
  savedDraft.value = current.value
    ? { ...current.value.config }
    : emptyConfig()
  draft.value = { ...savedDraft.value }
  username.value = ''
  password.value = ''
  bearerToken.value = ''
}

function switchNode(id: string): void {
  selectedId.value = id
  selectSource()
  if (saveOutcome.value !== 'refresh_failed')
    saveAction.clearFeedback()
  if (removeOutcome.value !== 'refresh_failed')
    removeAction.clearFeedback()
}

function requestNodeChange(id: string): void {
  if (id === selectedId.value || busy.value || loading.value)
    return
  if (hasUnsavedChanges.value) {
    discardFocusTarget = document.getElementById('traffic-source-node')
    pendingNodeId.value = id
  }
  else {
    switchNode(id)
  }
}

function settleLeave(leave: boolean): void {
  const resolve = resolveLeave
  resolveLeave = null
  pendingLeave.value = false
  resolve?.(leave)
}

function cancelDiscard(): void {
  pendingNodeId.value = null
  settleLeave(false)
}

function discardAndContinue(): void {
  if (pendingLeave.value) {
    settleLeave(true)
    return
  }
  const id = pendingNodeId.value
  pendingNodeId.value = null
  if (id !== null)
    switchNode(id)
}

function restoreDiscardFocus(event: Event): void {
  event.preventDefault()
  if (discardFocusTarget?.isConnected)
    discardFocusTarget.focus()
  discardFocusTarget = null
}

function confirmDeparture(to: RouteLocationNormalized): boolean | Promise<boolean> {
  const decision = decideTrafficDeparture({
    loggedIn: auth.loggedIn,
    stayingOnTrafficSection: to.name === 'admin' && to.query.section === 'traffic',
    hasUnsavedChanges: hasUnsavedChanges.value,
    busy: busy.value,
  })
  if (decision === 'allow') {
    if (!auth.loggedIn) {
      pendingNodeId.value = null
      settleLeave(true)
    }
    return true
  }
  if (decision === 'block')
    return false
  // Keep only one pending decision, including repeated browser Back attempts.
  cancelDiscard()
  discardFocusTarget = document.activeElement instanceof HTMLElement
    ? document.activeElement
    : null
  pendingLeave.value = true
  return new Promise<boolean>((resolve) => {
    resolveLeave = resolve
  })
}

watch(
  () => shouldWarnBeforeUnload(auth.loggedIn, hasUnsavedChanges.value),
  active => syncBeforeUnload(active),
  { immediate: true },
)

watch(() => auth.loggedIn, (loggedIn) => {
  if (!loggedIn) {
    syncBeforeUnload(false)
    pendingNodeId.value = null
    settleLeave(true)
  }
})

const unregisterDraft = draftRegistry?.register({
  section: 'traffic',
  isDirty: () => hasUnsavedChanges.value,
  isBusy: () => busy.value,
})

onBeforeRouteUpdate(confirmDeparture)
onBeforeRouteLeave(confirmDeparture)
onBeforeUnmount(() => {
  disposed = true
  unregisterDraft?.()
  syncBeforeUnload(false)
  settleLeave(unmountLeaveResult(auth.loggedIn))
})

onMounted(() => void load())

function clearPendingSave(nodeId: string): void {
  if (!pendingUncertainSaves.value[nodeId])
    return
  const next = { ...pendingUncertainSaves.value }
  delete next[nodeId]
  pendingUncertainSaves.value = next
}

function clearPendingRemove(nodeId: string): void {
  if (!pendingUncertainRemovals.value[nodeId])
    return
  const next = { ...pendingUncertainRemovals.value }
  delete next[nodeId]
  pendingUncertainRemovals.value = next
}

async function save(): Promise<void> {
  if (!selectedId.value || loading.value)
    return
  const targetId = selectedId.value
  const targetName = currentNodeName.value
  const config = { ...draft.value }
  const hadCredentialBefore = Boolean(current.value?.has_credential)
  const includedCredential = Boolean(username.value || password.value || bearerToken.value)
  clearPendingSave(targetId)
  const result = await saveAction.run(async () => {
    await getSharedApi().post(`admin/traffic-sources/${encodeURIComponent(targetId)}`, {
      config,
      username: username.value,
      password: password.value,
      bearer_token: bearerToken.value,
    })
  }, {
    message: '已保存；启用后通常在一分钟内首次同步',
    onCommitted: () => {
      username.value = ''
      password.value = ''
      bearerToken.value = ''
      savedDraft.value = config
    },
  })
  if (result === 'uncertain') {
    pendingUncertainSaves.value = {
      ...pendingUncertainSaves.value,
      [targetId]: {
        nodeId: targetId,
        nodeName: targetName,
        config,
        hadCredentialBefore,
        includedCredential,
        message: saveError.value,
        resyncError: '',
      },
    }
  }
}

function openRemove(): void {
  if (removeOutcome.value === 'failed' || removeOutcome.value === 'saved')
    removeAction.clearFeedback()
  confirmRemove.value = true
}

function closeRemove(): void {
  if (removeBusy.value)
    return
  confirmRemove.value = false
  if (removeOutcome.value === 'failed')
    removeAction.clearFeedback()
}

async function remove(): Promise<void> {
  if (!selectedId.value || loading.value || currentPendingRemove.value)
    return
  const targetId = selectedId.value
  const targetName = currentNodeName.value
  const result = await removeAction.run(async () => {
    await getSharedApi().post(`admin/traffic-sources/${encodeURIComponent(targetId)}/delete`, {})
  }, {
    message: '已移除外部流量源',
    riskyWrite: true,
    uncertainMessage: detail => `移除节点「${targetName}」外部流量源结果未确认（${detail}）。请先重新同步核对当前状态，勿直接重复提交。`,
    onCommitted: () => {
      clearPendingRemove(targetId)
      sources.value = sources.value.filter(source => source.node_id !== targetId)
      selectSource()
      confirmRemove.value = false
    },
  })
  if (result === 'uncertain') {
    pendingUncertainRemovals.value = {
      ...pendingUncertainRemovals.value,
      [targetId]: {
        nodeId: targetId,
        nodeName: targetName,
        message: removeError.value,
        resyncError: '',
      },
    }
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
      const fresh = sources.value.find(item => item.node_id === pending.nodeId)
      clearPendingSave(pending.nodeId)
      const configMatches = Boolean(fresh && sameTrafficConfig(fresh.config, pending.config))
      if (configMatches && (!pending.includedCredential || (!pending.hadCredentialBefore && fresh?.has_credential))) {
        if (selectedId.value === pending.nodeId) {
          savedDraft.value = { ...pending.config }
          username.value = ''
          password.value = ''
          bearerToken.value = ''
        }
        return {
          resolved: true,
          message: `已核实：节点「${pending.nodeName}」的外部流量源配置已在服务端生效`,
        }
      }
      if (configMatches && pending.includedCredential && pending.hadCredentialBefore) {
        if (selectedId.value === pending.nodeId)
          savedDraft.value = { ...pending.config }
        return {
          resolved: true,
          message: `已同步最新状态：节点「${pending.nodeName}」的配置字段与刚才提交一致，但服务端不会回显凭据内容，无法自动核实新凭据是否已更新；请核对后再决定是否重新保存。`,
        }
      }
      return {
        resolved: true,
        message: `已同步最新状态：节点「${pending.nodeName}」的服务端配置与刚才提交不一致，已保留当前输入，请核对后再决定是否重新保存。`,
      }
    },
  })
  if (!ok && pendingUncertainSaves.value[pending.nodeId])
    pendingUncertainSaves.value[pending.nodeId]!.resyncError = saveResyncError.value
}

async function resyncRemove(): Promise<void> {
  const pending = currentPendingRemove.value
  if (!pending) {
    await removeAction.resync('已重新同步最新状态')
    return
  }
  pending.resyncError = ''
  const ok = await removeAction.resync({
    verify: () => {
      const stillExists = sources.value.some(source => source.node_id === pending.nodeId)
      clearPendingRemove(pending.nodeId)
      if (!stillExists) {
        if (selectedId.value === pending.nodeId)
          selectSource()
        confirmRemove.value = false
        return {
          resolved: true,
          message: `已核实：已移除节点「${pending.nodeName}」的外部流量源`,
        }
      }
      return {
        resolved: true,
        message: `已核对最新状态：节点「${pending.nodeName}」的外部流量源仍存在，可确认后再决定是否重试移除。`,
      }
    },
  })
  if (!ok && pendingUncertainRemovals.value[pending.nodeId])
    pendingUncertainRemovals.value[pending.nodeId]!.resyncError = removeResyncError.value
}

async function resyncPage(): Promise<void> {
  if (currentPendingSave.value || saveNeedsResync.value) {
    await resyncSave()
    return
  }
  await resyncRemove()
}

const statusText: Record<string, string> = {
  ok: '同步正常',
  pending: '等待首次同步',
  authentication_required: '认证失败，需管理员处理',
  provider_unavailable: '服务商暂时不可用',
  provider_http_error: '服务商返回错误',
  invalid_data: '返回字段无法解析',
  response_too_large: '响应超过上限',
  credential_unavailable: '凭据不可用',
  private_address_rejected: '目标地址不安全',
  invalid_configuration: '配置无效',
}
</script>

<template>
  <div class="space-y-4">
    <div v-if="pageAlert || pageNeedsResync" class="flex flex-wrap items-center gap-2">
      <p v-if="pageAlert" role="alert" class="text-sm text-destructive">
        {{ pageAlert }}
      </p>
      <Button v-if="pageNeedsResync" type="button" variant="outline" size="sm" :disabled="busy" @click="resyncPage">
        重新同步
      </Button>
    </div>
    <p v-if="pageSuccess" role="status" class="text-sm text-success">
      {{ pageSuccess }}
    </p>
    <CardX title="外部流量源" content-class="space-y-4">
      <p class="text-sm text-muted-foreground">
        仅 Service 定时读取管理员指定的 HTTPS JSON 接口；外部快照与 Agent 本地流量分别保存，不会相互覆盖。默认每 30 分钟同步，失败时保留上次值并显示过期状态。
      </p>
      <p v-if="!keyReady" role="alert" class="rounded-md border border-destructive/40 p-3 text-sm text-destructive">
        尚未配置 SENLUN_TRAFFIC_KEY_FILE。请在 Service 上提供权限为 0600 的 32 字节随机密钥文件后重启，才能保存登录凭据。
      </p>
      <Button type="button" variant="outline" :disabled="loading || busy" @click="load()">
        {{ loading ? '正在刷新…' : '刷新同步状态' }}
      </Button>
      <label class="grid gap-2 text-sm">关联节点
        <SelectControl id="traffic-source-node" aria-label="关联节点" :model-value="selectedId" :options="nodes.map(node => ({ value: node.id, label: node.name }))" :disabled="busy || loading" @update:model-value="requestNodeChange" />
      </label>
      <p v-if="hasUnsavedChanges" role="status" class="text-sm text-muted-foreground">
        有未保存修改；刷新同步状态不会清空输入。凭据只保留在当前页面内存中。
      </p>
      <p v-if="!nodes.length" class="text-sm text-muted-foreground">
        请先注册节点。
      </p>
      <div v-if="current" class="rounded-md border p-3 text-sm">
        <p>状态：{{ statusText[current.status] ?? current.status }}</p>
        <p v-if="current.last_success_unix_ms">
          上次成功：{{ new Date(current.last_success_unix_ms).toLocaleString() }}
        </p>
        <p v-if="current.snapshot">
          上次已用：{{ (current.snapshot.used_bytes / 1024 ** 3).toFixed(2) }} GiB
        </p>
        <p v-if="current.status !== 'ok' && current.snapshot" class="text-amber-600">
          该数值为上次成功快照，不代表当前流量。
        </p>
      </div>
      <form v-if="selectedId" class="space-y-5" :aria-busy="busy" @submit.prevent="save">
        <fieldset :disabled="busy || !keyReady" class="grid gap-4 sm:grid-cols-2">
          <legend class="mb-3 font-medium">
            查询配置
          </legend>
          <label class="grid gap-2 text-sm">来源名称（必填）<Input v-model="draft.name" required maxlength="128" placeholder="例如：我的服务商" /></label>
          <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="draft.enabled" type="checkbox" class="size-4 accent-primary">启用定时同步</label>
          <label class="grid gap-2 text-sm sm:col-span-2">流量查询网址（HTTPS，GET，必填）<Input v-model="draft.traffic_url" type="url" required maxlength="2048" autocomplete="off" placeholder="https://example.com/api/usage" /></label>
          <label class="grid gap-2 text-sm">登录方式
            <SelectControl v-model="draft.auth_mode" aria-label="登录方式" :disabled="busy || !keyReady" :options="[{ value: 'none', label: '无需认证' }, { value: 'bearer', label: '固定 Bearer Token' }, { value: 'login_refresh', label: '登录 + Refresh Token' }]" />
          </label>
          <label v-if="draft.auth_mode === 'bearer'" class="grid gap-2 text-sm">Bearer Token（留空保留已存凭据）<Input v-model="bearerToken" type="password" autocomplete="new-password" :required="!canReuseCredential" /></label>
        </fieldset>
        <fieldset v-if="draft.auth_mode === 'login_refresh'" :disabled="busy || !keyReady" class="grid gap-4 sm:grid-cols-2">
          <legend class="mb-3 font-medium">
            登录与令牌刷新
          </legend>
          <label class="grid gap-2 text-sm sm:col-span-2">登录网址（HTTPS，POST，必填）<Input v-model="draft.login_url" type="url" required maxlength="2048" autocomplete="off" /></label>
          <label class="grid gap-2 text-sm sm:col-span-2">刷新网址（HTTPS，POST，必填）<Input v-model="draft.refresh_url" type="url" required maxlength="2048" autocomplete="off" /></label>
          <label class="grid gap-2 text-sm">账号字段名<Input v-model="draft.username_field" required maxlength="64" /></label>
          <label class="grid gap-2 text-sm">密码字段名<Input v-model="draft.password_field" required maxlength="64" /></label>
          <label class="grid gap-2 text-sm">刷新令牌字段名<Input v-model="draft.refresh_token_field" required maxlength="64" /></label>
          <label class="grid gap-2 text-sm">账号（更新时与密码一同填写）<Input v-model="username" autocomplete="off" :required="!canReuseCredential" /></label>
          <label class="grid gap-2 text-sm">密码（留空保留已存凭据）<Input v-model="password" type="password" autocomplete="new-password" :required="!canReuseCredential" /></label>
          <label class="grid gap-2 text-sm">访问令牌 JSON Pointer<Input v-model="draft.access_pointer" required placeholder="/data/access_token" /></label>
          <label class="grid gap-2 text-sm">刷新令牌 JSON Pointer<Input v-model="draft.refresh_pointer" required placeholder="/data/refresh_token" /></label>
        </fieldset>
        <fieldset :disabled="busy || !keyReady" class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <legend class="mb-3 font-medium">
            流量字段映射
          </legend>
          <p class="text-sm text-muted-foreground sm:col-span-2 lg:col-span-3">
            字段路径使用 JSON Pointer，例如 /data/usage/in。上行、下行及已用量均为原始字节整数；已用量路径留空时按上行加下行计算。
          </p>
          <label class="grid gap-2 text-sm">上行字节路径（必填）<Input v-model="draft.up_pointer" required /></label>
          <label class="grid gap-2 text-sm">下行字节路径（必填）<Input v-model="draft.down_pointer" required /></label>
          <label class="grid gap-2 text-sm">计费已用路径（可选）<Input v-model="draft.used_pointer" /></label>
          <label class="grid gap-2 text-sm">额度路径（可选）<Input v-model="draft.limit_pointer" /></label>
          <label class="grid gap-2 text-sm">额度单位
            <SelectControl v-model="draft.limit_unit" aria-label="额度单位" :disabled="busy || !keyReady" :options="[{ value: 'bytes', label: '字节' }, { value: 'gb', label: 'GB（1000³）' }, { value: 'gib', label: 'GiB（1024³）' }]" />
          </label>
          <label class="grid gap-2 text-sm">周期重置时间路径（可选）<Input v-model="draft.reset_pointer" /></label>
        </fieldset>
        <div class="flex flex-wrap gap-2">
          <Button type="submit" class="min-h-11" :disabled="busy || loading || !keyReady">
            {{ saveBusy ? '正在保存…' : '保存外部流量源' }}
          </Button>
          <Button v-if="current" type="button" variant="outline" class="min-h-11 text-destructive" :disabled="busy || loading" @click="openRemove">
            移除流量源
          </Button>
        </div>
      </form>
    </CardX>
    <Dialog :open="pendingNodeId !== null || pendingLeave" @update:open="value => { if (!value) cancelDiscard() }">
      <DialogContent @close-auto-focus="restoreDiscardFocus">
        <DialogHeader>
          <DialogTitle>放弃未保存修改？</DialogTitle>
          <DialogDescription v-if="pendingLeave">
            离开外部流量栏目会丢弃当前配置草稿及尚未保存的凭据输入，不会保存任何修改。
          </DialogDescription>
          <DialogDescription v-else>
            切换到 {{ pendingNodeName }} 会丢弃当前配置草稿及尚未保存的凭据输入，不会保存任何修改。
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" @click="cancelDiscard">
            取消，继续编辑
          </Button>
          <Button variant="destructive" @click="discardAndContinue">
            {{ pendingLeave ? '放弃修改并离开' : '放弃修改并切换' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
    <Dialog :open="confirmRemove" @update:open="value => { if (!value) closeRemove() }">
      <DialogContent :show-close="!removeBusy" @escape-key-down="event => { if (removeBusy) event.preventDefault() }" @interact-outside="event => { if (removeBusy) event.preventDefault() }">
        <DialogHeader>
          <DialogTitle>移除外部流量源？</DialogTitle>
          <DialogDescription>
            将删除节点「{{ currentNodeName }}」的外部流量配置、凭据及快照；Agent 本地流量不会改变。
          </DialogDescription>
        </DialogHeader>
        <p v-if="currentPendingRemove" role="alert" class="text-sm text-destructive">
          {{ formatPendingAlert(currentPendingRemove.message, currentPendingRemove.resyncError) }}
        </p>
        <p v-else-if="removeError" role="alert" class="text-sm text-destructive">
          {{ removeError }}
        </p>
        <p v-if="!currentPendingRemove && removeSuccess" role="status" class="text-sm text-success">
          {{ removeSuccess }}
        </p>
        <DialogFooter>
          <Button variant="outline" :disabled="removeBusy" @click="closeRemove">
            取消
          </Button>
          <Button v-if="currentPendingRemove || removeNeedsResync" variant="outline" :disabled="removeBusy" @click="resyncRemove">
            重新同步
          </Button>
          <Button variant="destructive" :disabled="removeBusy || Boolean(currentPendingRemove)" @click="remove">
            {{ removeBusy ? '正在移除…' : '确认移除' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
