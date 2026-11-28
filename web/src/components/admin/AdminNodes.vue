<script setup lang="ts">
import type { RouteLocationNormalized } from 'vue-router'
import type { ManagedNode } from '@/utils/admin'
import type { NodeDraftForm, NodeDraftSnapshot } from '@/utils/adminDraft'
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
import { getSharedApi } from '@/utils/api'
import { trafficSelectionDetails } from '@/utils/nodeHelpers'
import { parseTags } from '@/utils/tagHelper'

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
const confirm = ref<'rotate' | 'revoke' | 'delete' | 'reset-traffic' | null>(null)
const newTagText = ref('')
const newTagColor = ref<TagColor>('blue')
const tagError = ref('')
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

const saveAction = useAdminAction(props.refresh)
const dangerAction = useAdminAction(props.refresh)
const busy = computed(() => saveAction.busy.value || dangerAction.busy.value)
const hasUnsavedChanges = computed(() => isNodeDraftDirty({
  draft: draft.value,
  expires: expires.value,
  quotaGiB: quotaGiB.value,
  newTagText: newTagText.value,
}, savedBaseline.value))

const pageError = computed(() => saveAction.error.value || (confirm.value === null ? dangerAction.error.value : ''))
const pageSuccess = computed(() => saveAction.success.value || dangerAction.success.value)
const pageNeedsResync = computed(() => saveAction.needsResync.value || (confirm.value === null && dangerAction.needsResync.value))

const actionLabels = {
  'rotate': '轮换 Agent 凭据',
  'revoke': '撤销 Agent 凭据',
  'delete': '删除节点',
  'reset-traffic': '重置本周期流量',
}
const actionHints = {
  'rotate': '旧凭据会立即失效。请保存新凭据并更新对应 Agent，否则节点将停止上报。',
  'revoke': '对应 Agent 将无法继续上报。需要重新注册后才能恢复。',
  'delete': '将删除此节点及相关数据，无法从面板恢复。',
  'reset-traffic': '将本周期流量计数归零，不会重置主机的网络接口计数。',
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

const unregisterDraft = draftRegistry?.register({
  section: 'nodes',
  isDirty: () => hasUnsavedChanges.value,
  isBusy: () => busy.value,
  requestRefresh: triggerEl => draftGuard.requestRefresh(async () => {
    forceResetNextSync = true
    saveAction.clearFeedback()
    dangerAction.clearFeedback()
    try {
      await props.refresh()
    }
    catch (cause) {
      forceResetNextSync = false
      saveAction.error.value = cause instanceof Error
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

function openConfirm(action: 'rotate' | 'revoke' | 'delete' | 'reset-traffic'): void {
  dangerAction.clearFeedback()
  confirm.value = action
}

function closeConfirm(): void {
  if (busy.value)
    return
  confirm.value = null
  dangerAction.clearFeedback()
}

async function save(): Promise<void> {
  if (!draft.value)
    return
  const remarkError = validatePublicRemark(draft.value.public_remark)
  if (remarkError) {
    saveAction.error.value = remarkError
    saveAction.success.value = ''
    return
  }
  const normalizedRemark = normalizePublicRemark(draft.value.public_remark)
  const numericQuota = Number(quotaGiB.value) || 0
  const node = {
    ...draft.value,
    public_remark: normalizedRemark,
    expired_at_unix_ms: expires.value
      ? new Date(expires.value).getTime()
      : null,
    traffic_limit_bytes: Math.round(numericQuota * 1024 ** 3),
  }
  const submittedExpires = expires.value
  await saveAction.run(async () => {
    await getSharedApi().post(`admin/nodes/${encodeURIComponent(node.id)}`, node)
  }, {
    message: '已保存',
    onCommitted: () => {
      if (!draft.value)
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
}

async function enroll(): Promise<void> {
  secret.value = ''
  await saveAction.run(async () => {
    const response = await getSharedApi().post<{ token: string }>('admin/enrollment', { ttl_seconds: ttlHours.value * 3600 })
    secret.value = response.token
  }, {
    message: '已生成注册令牌',
    reload: false,
    riskyWrite: true,
  })
}

async function execute(): Promise<void> {
  if (!confirm.value || !selected.value || dangerAction.uncertain.value)
    return
  const action = confirm.value
  const id = selected.value.id
  secret.value = ''
  await dangerAction.run(async () => {
    if (action === 'rotate') {
      const response = await getSharedApi().post<{ agent_token: string }>(`admin/nodes/${encodeURIComponent(id)}/rotate`)
      secret.value = response.agent_token
    }
    else {
      await getSharedApi().post(`admin/nodes/${encodeURIComponent(id)}/${action}`)
    }
  }, {
    message: '操作已完成',
    riskyWrite: true,
    onCommitted: () => {
      confirm.value = null
    },
  })
}

async function resyncPage(): Promise<void> {
  if (saveAction.needsResync.value) {
    await saveAction.resync()
    return
  }
  await dangerAction.resync()
}

async function resyncDanger(): Promise<void> {
  const ok = await dangerAction.resync()
  if (ok)
    confirm.value = null
}
</script>

<template>
  <div class="space-y-4">
    <div v-if="pageError || pageNeedsResync" class="flex flex-wrap items-center gap-2">
      <p v-if="pageError" role="alert" class="text-sm text-destructive">
        {{ pageError }}
      </p>
      <Button v-if="pageNeedsResync" type="button" variant="outline" size="sm" :disabled="busy" @click="resyncPage">
        重新同步
      </Button>
    </div>
    <p v-if="pageSuccess" role="status" class="text-sm text-success">
      {{ pageSuccess }}
    </p>
    <CardX title="接入节点" content-class="space-y-4">
      <form class="flex flex-wrap items-end gap-3" @submit.prevent="enroll">
        <label class="grid gap-2 text-sm">注册令牌有效期（小时）<Input v-model="ttlHours" type="number" min="1" max="24" required class="max-w-48" :disabled="busy" /></label>
        <Button type="submit" class="min-h-11" :disabled="busy">
          生成一次性注册令牌
        </Button>
      </form>
      <p class="text-sm text-muted-foreground">
        令牌仅用于注册一个 Agent，请通过受保护的配置文件交给对应服务器。
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
            {{ saveAction.busy.value ? '正在处理…' : '保存节点' }}
          </Button><Button variant="outline" type="button" class="min-h-11" :disabled="selected?.hidden" @click="router.push(`/instance/${draft.id}`)">
            查看监控
          </Button>
          <Button v-if="saveAction.needsResync.value" variant="outline" type="button" class="min-h-11" :disabled="busy" @click="resyncPage">
            重新同步
          </Button>
        </div>
        <p v-if="selected?.hidden" class="text-sm text-muted-foreground">
          此节点已从监控页面和探测历史中隐藏。取消隐藏并保存后可查看。
        </p>
      </form>
      <div v-if="draft" class="flex flex-wrap gap-2 border-t pt-4">
        <AdminTrafficCorrection v-if="selected" :node="selected" :disabled="busy" :refresh="props.refresh" />
        <Button v-for="(label, action) in actionLabels" :key="action" variant="outline" type="button" :disabled="busy" :class="action === 'delete' ? 'text-destructive' : ''" @click="openConfirm(action)">
          {{ label }}
        </Button>
      </div>
    </CardX>
    <Dialog :open="draftGuard.isOpen.value" @update:open="value => { if (!value) draftGuard.cancel() }">
      <DialogContent @close-auto-focus="draftGuard.restoreFocus">
        <DialogHeader>
          <DialogTitle>放弃未保存修改？</DialogTitle>
          <DialogDescription v-if="draftGuard.pending.value?.kind === 'switch'">
            切换到 {{ draftGuard.pending.value.targetLabel }} 会丢弃当前节点未保存的修改，不会保存任何更改。
          </DialogDescription>
          <DialogDescription v-else-if="draftGuard.pending.value?.kind === 'refresh'">
            刷新管理数据会丢弃当前节点未保存的修改，不会保存任何更改。
          </DialogDescription>
          <DialogDescription v-else>
            离开当前节点编辑会丢弃未保存的修改，不会保存任何更改。
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" @click="draftGuard.cancel">
            取消，继续编辑
          </Button>
          <Button variant="destructive" @click="draftGuard.confirm">
            {{
              draftGuard.pending.value?.kind === 'switch'
                ? '放弃修改并切换'
                : draftGuard.pending.value?.kind === 'refresh'
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
        <p v-if="dangerAction.error.value" role="alert" class="text-sm text-destructive">
          {{ dangerAction.error.value }}
        </p>
        <DialogFooter>
          <Button variant="outline" :disabled="busy" @click="closeConfirm">
            取消
          </Button>
          <Button v-if="dangerAction.needsResync.value" variant="outline" :disabled="busy" @click="resyncDanger">
            重新同步
          </Button>
          <Button variant="destructive" :disabled="busy || dangerAction.uncertain.value" @click="execute">
            {{ dangerAction.busy.value ? '正在处理…' : '确认执行' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
