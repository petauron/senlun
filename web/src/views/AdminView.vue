<script setup lang="ts">
import type { RouteLocationNormalized } from 'vue-router'
import type { ActiveSectionDraft } from '@/composables/useAdminDraftGuard'
import type { AdminState, SiteSettings } from '@/utils/admin'
import { computed, nextTick, onBeforeUnmount, onMounted, provide, ref, shallowRef, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AccountSecurity from '@/components/admin/AccountSecurity.vue'
import AdminAlerts from '@/components/admin/AdminAlerts.vue'
import AdminNodes from '@/components/admin/AdminNodes.vue'
import AdminProbes from '@/components/admin/AdminProbes.vue'
import AdminThemes from '@/components/admin/AdminThemes.vue'
import AdminTrafficSources from '@/components/admin/AdminTrafficSources.vue'
import { Button } from '@/components/ui/button'
import { CardX } from '@/components/ui/card-x'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import SelectControl from '@/components/ui/SelectControl.vue'
import { useAdminAction } from '@/composables/useAdminAction'
import { ADMIN_DRAFT_REGISTRY_KEY, useAdminDraftGuard } from '@/composables/useAdminDraftGuard'
import { useAppStore } from '@/stores/app'
import { useAuthStore } from '@/stores/auth'
import { isSiteSettingsDirty, normalizeSiteSettings } from '@/utils/adminDraft'
import { createSerializedRefresh } from '@/utils/adminRefresh'
import { ApiError, getSharedApi } from '@/utils/api'
import { destroyInitManager } from '@/utils/init'

interface RegisteredSectionDraft extends ActiveSectionDraft {
  id: number
}

interface PendingUncertainSiteSave {
  submitted: SiteSettings
  message: string
  resyncError: string
}

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const app = useAppStore()
const state = shallowRef<AdminState | null>(null)
const settings = ref<SiteSettings>(normalizeSiteSettings({
  site_name: 'Senlun',
  private_site: true,
  agent_interval_seconds: 3,
  peer_probe_interval_seconds: 60,
  earth_view_mode: 'maps',
}))
const savedSettings = ref<SiteSettings | null>(null)
const pendingUncertainSettings = ref<PendingUncertainSiteSave | null>(null)
const loading = ref(false)
const loadError = ref('')
const activeSectionDraft = shallowRef<RegisteredSectionDraft | null>(null)
let nextDraftRegistrationId = 0
let controller: AbortController | null = null
let disposed = false

provide(ADMIN_DRAFT_REGISTRY_KEY, {
  register(entry: ActiveSectionDraft) {
    const registered: RegisteredSectionDraft = {
      ...entry,
      id: ++nextDraftRegistrationId,
    }
    activeSectionDraft.value = registered
    return () => {
      if (activeSectionDraft.value?.id === registered.id)
        activeSectionDraft.value = null
    }
  },
})

const sections = [
  { id: 'nodes', label: '节点' },
  { id: 'traffic', label: '外部流量' },
  { id: 'probes', label: '探测' },
  { id: 'alerts', label: '通知与告警' },
  { id: 'themes', label: '主题' },
  { id: 'site', label: '站点设置' },
  { id: 'security', label: '账号安全' },
]
const section = computed(() => sections.some(item => item.id === route.query.section)
  ? String(route.query.section)
  : 'nodes')
const hasUnsavedSettings = computed(() => isSiteSettingsDirty(settings.value, savedSettings.value))
const currentSectionDraft = computed(() => {
  const registered = activeSectionDraft.value
  if (!registered || registered.section !== section.value)
    return null
  return registered
})

const stateRefresher = createSerializedRefresh<void, { forceResetSite: boolean, surfaceLoadError: boolean }>(
  async (options, isLatest) => {
    if (disposed)
      return
    if (!auth.loggedIn)
      throw new ApiError('登录状态已失效，无法同步最新状态', 401)
    if (options.surfaceLoadError)
      loadError.value = ''
    controller = new AbortController()
    try {
      const response = await getSharedApi().get<AdminState>('admin/state', controller.signal)
      if (!disposed && isLatest()) {
        loadError.value = ''
        state.value = response
        const normalized = normalizeSiteSettings(response.settings)
        if (options.forceResetSite || !savedSettings.value || !hasUnsavedSettings.value) {
          settings.value = { ...normalized }
          savedSettings.value = { ...normalized }
        }
        else if (!isSiteSettingsDirty(settings.value, normalized)) {
          savedSettings.value = { ...normalized }
        }
        if (app.publicSettings) {
          app.publicSettings = {
            ...app.publicSettings,
            sitename: response.settings.site_name,
            private_site: response.settings.private_site,
            theme_settings: {
              ...app.publicSettings.theme_settings,
              dataUpdateInterval: response.settings.agent_interval_seconds,
              dailyExchangeRates: response.settings.daily_exchange_rates,
              earthViewMode: normalized.earth_view_mode,
            },
          }
        }
        await nextTick()
      }
    }
    catch (cause) {
      if (!disposed && options.surfaceLoadError && isLatest() && auth.loggedIn) {
        loadError.value = cause instanceof Error
          ? cause.message
          : String(cause)
      }
      throw cause
    }
  },
  (nextLoading) => {
    loading.value = nextLoading
  },
)

async function refresh(forceResetSite = false, surfaceLoadError = true): Promise<void> {
  await stateRefresher.refresh({
    forceResetSite,
    surfaceLoadError,
  })
}

const settingsAction = useAdminAction(() => refresh(false, false))
const logoutAction = useAdminAction()
const {
  busy: settingsBusy,
  error: settingsError,
  resyncError: settingsResyncError,
  success: settingsSuccess,
  needsResync: settingsNeedsResync,
} = settingsAction
const {
  busy: logoutBusy,
  error: logoutError,
} = logoutAction

const busy = computed(() => settingsBusy.value || logoutBusy.value)
const topRefreshDisabled = computed(() => loading.value || busy.value || (currentSectionDraft.value?.isBusy() ?? false))
const settingsAlert = computed(() => {
  const pending = pendingUncertainSettings.value
  if (pending) {
    return pending.resyncError
      ? `${pending.message}（重新同步失败：${pending.resyncError}）`
      : pending.message
  }
  return settingsError.value
})
const showSettingsResync = computed(() => settingsNeedsResync.value || Boolean(pendingUncertainSettings.value))

function isStayingOnSiteSection(to: RouteLocationNormalized): boolean {
  if (section.value !== 'site')
    return true
  return to.name === 'admin' && to.query.section === 'site'
}

const siteDraftGuard = useAdminDraftGuard({
  isLoggedIn: () => auth.loggedIn,
  isDirty: () => section.value === 'site' && hasUnsavedSettings.value,
  isBusy: () => settingsBusy.value,
  isStayingInSameContext: isStayingOnSiteSection,
})
const {
  pending: siteGuardPending,
  isOpen: siteGuardOpen,
  cancel: cancelSiteGuard,
  confirm: confirmSiteGuard,
  restoreFocus: restoreSiteGuardFocus,
} = siteDraftGuard

function resetSiteDraftToSaved(): void {
  if (savedSettings.value)
    settings.value = { ...savedSettings.value }
  settingsAction.clearFeedback()
  pendingUncertainSettings.value = null
}

watch(section, (nextSection, prevSection) => {
  if (prevSection === 'site' && nextSection !== 'site')
    resetSiteDraftToSaved()
})

function requestTopRefresh(event: MouseEvent): void {
  if (topRefreshDisabled.value)
    return
  const triggerEl = event.currentTarget instanceof HTMLElement
    ? event.currentTarget
    : null
  if (currentSectionDraft.value?.requestRefresh?.(triggerEl))
    return
  if (section.value === 'site' && siteDraftGuard.requestRefresh(() => {
    settingsAction.clearFeedback()
    pendingUncertainSettings.value = null
    void refresh(true).catch(() => {})
  }, triggerEl)) {
    return
  }
  void refresh(section.value === 'site').catch(() => {})
}

onMounted(() => void refresh(true).catch(() => {}))
onBeforeUnmount(() => {
  disposed = true
  controller?.abort()
  state.value = null
})

async function saveSettings(): Promise<void> {
  const submitted = normalizeSiteSettings(settings.value)
  pendingUncertainSettings.value = null
  const result = await settingsAction.run(async () => {
    await getSharedApi().post('admin/settings', submitted)
  }, {
    message: '已保存',
    onCommitted: () => {
      settings.value = { ...submitted }
      savedSettings.value = { ...submitted }
    },
  })
  if (result === 'uncertain') {
    pendingUncertainSettings.value = {
      submitted,
      message: settingsError.value,
      resyncError: '',
    }
  }
}

async function resyncSettings(): Promise<void> {
  const pending = pendingUncertainSettings.value
  if (!pending) {
    await settingsAction.resync('已重新同步最新状态')
    return
  }
  pending.resyncError = ''
  const ok = await settingsAction.resync({
    verify: () => {
      if (!state.value) {
        return {
          resolved: false,
          error: pending.message,
        }
      }
      const serverSettings = normalizeSiteSettings(state.value.settings)
      pendingUncertainSettings.value = null
      if (!isSiteSettingsDirty(pending.submitted, serverSettings)) {
        savedSettings.value = { ...serverSettings }
        const editedAfterSubmit = isSiteSettingsDirty(settings.value, pending.submitted)
        return {
          resolved: true,
          message: editedAfterSubmit
            ? '已核实：上次提交的站点设置已在服务端生效；您之后的新修改已保留，尚未保存。'
            : '已核实：站点设置已在服务端生效',
        }
      }
      return {
        resolved: true,
        message: '已同步最新状态：服务端站点设置与刚才提交不一致，已保留当前输入，请核对后再决定是否重新保存。',
      }
    },
  })
  if (!ok && pendingUncertainSettings.value)
    pendingUncertainSettings.value.resyncError = settingsResyncError.value
}

function handleNodeDeleted(deletedId: string): void {
  if (!state.value)
    return
  if (!state.value.nodes.some(node => node.id === deletedId))
    return
  state.value = {
    ...state.value,
    nodes: state.value.nodes.filter(node => node.id !== deletedId),
  }
}

async function logout(): Promise<void> {
  await logoutAction.run(async () => {
    await auth.logout()
    destroyInitManager()
    state.value = null
    await router.replace('/login')
  }, {
    message: '',
    reload: false,
  })
}
</script>

<template>
  <div class="space-y-4 px-4 pb-8">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="text-xl font-semibold">
          管理 Senlun
        </h1><p class="text-sm text-muted-foreground">
          {{ auth.status?.username }}
        </p>
      </div><div class="flex gap-2">
        <Button variant="outline" :disabled="topRefreshDisabled" @click="requestTopRefresh">
          {{ loading ? '正在刷新…' : '刷新' }}
        </Button><Button variant="outline" :disabled="logoutBusy" @click="logout">
          退出登录
        </Button>
      </div>
    </div>
    <nav aria-label="管理页面" class="flex flex-wrap gap-2">
      <RouterLink v-for="item in sections" :key="item.id" :to="{ name: 'admin', query: { section: item.id } }" :aria-current="section === item.id ? 'page' : undefined" class="inline-flex min-h-11 items-center rounded-md px-3 text-sm transition-colors focus-visible:ring-2 focus-visible:ring-ring" :class="section === item.id ? 'bg-primary text-primary-foreground' : 'bg-background/60 hover:bg-accent'">
        {{ item.label }}
      </RouterLink>
    </nav>
    <p v-if="loadError" role="alert" class="rounded-md border border-destructive/40 p-3 text-sm text-destructive">
      {{ loadError }}
    </p>
    <p v-if="logoutError" role="alert" class="text-sm text-destructive">
      {{ logoutError }}
    </p>
    <p v-if="!state && loading" role="status" class="text-sm text-muted-foreground">
      正在读取管理数据…
    </p>
    <template v-if="state">
      <AdminNodes v-if="section === 'nodes'" :nodes="state.nodes" :refresh="() => refresh(false, false)" @node-deleted="handleNodeDeleted" />
      <AdminTrafficSources v-else-if="section === 'traffic'" :nodes="state.nodes" />
      <AdminProbes v-else-if="section === 'probes'" :nodes="state.nodes" :probes="state.probes" :refresh="() => refresh(false, false)" />
      <AdminAlerts v-else-if="section === 'alerts'" :nodes="state.nodes" :channels="state.channels" :rules="state.alert_rules" :incidents="state.incidents" :failures="state.notification_failures" :refresh="() => refresh(false, false)" />
      <AccountSecurity v-else-if="section === 'security'" />
      <AdminThemes v-else-if="section === 'themes'" />
      <CardX v-else title="站点设置" content-class="space-y-4">
        <p v-if="hasUnsavedSettings" role="status" class="text-sm text-muted-foreground">
          有未保存修改；离开或主动刷新前会提示确认。
        </p>
        <form class="max-w-xl space-y-4" :aria-busy="settingsBusy" @submit.prevent="saveSettings">
          <fieldset :disabled="settingsBusy" class="space-y-4">
            <label class="grid gap-2 text-sm">站点名称<Input v-model="settings.site_name" required maxlength="128" /></label>
            <label class="grid gap-2 text-sm">Agent 上报间隔（秒）<Input v-model="settings.agent_interval_seconds" type="number" min="1" max="300" step="1" required /></label>
            <label class="grid gap-2 text-sm">节点互测间隔（秒）<Input v-model="settings.peer_probe_interval_seconds" type="number" min="5" max="300" step="1" required /></label>
            <label class="grid gap-2 text-sm">首页头部展示模式
              <SelectControl v-model="settings.earth_view_mode" aria-label="首页头部展示模式" :disabled="settingsBusy" :options="[{ value: 'maps', label: '平面地图' }, { value: 'cards', label: '仅汇总卡片' }, { value: 'hide', label: '隐藏头部' }]" />
            </label>
            <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="settings.private_site" type="checkbox" class="size-4 accent-primary">私有站点：登录后才能查看监控数据</label>
            <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="settings.ip_info_enabled" type="checkbox" class="size-4 accent-primary" aria-describedby="ip-info-privacy">启用 IP 信息查询（仅管理员）</label>
            <label class="flex min-h-11 items-center gap-2 text-sm"><input v-model="settings.daily_exchange_rates" type="checkbox" class="size-4 accent-primary">每日汇率（Frankfurter）</label>
            <p class="text-sm text-muted-foreground">
              启用后，访客浏览器每天从 Frankfurter 获取一次汇率，不发送节点、价格或 Senlun 凭据；第三方会看到访客 IP。保存后刷新首页生效。
            </p>
            <p id="ip-info-privacy" class="text-sm text-muted-foreground">
              默认关闭。启用后，管理员查看 IP 信息时会将已登记的节点公网 IP 发送给 Net.Coffee，查询归属、原生性与全球延迟。不发送 Senlun 凭据或主机指标；第三方结果可能不准确或暂时不可用。节点地址请在“节点”中登记，中国大陆节点不查询。
            </p>
            <p class="text-sm text-muted-foreground">
              关闭私有站点后，未隐藏节点的监控数据对访客公开。
            </p>
          </fieldset>
          <div class="flex flex-wrap items-center gap-2">
            <Button type="submit" class="min-h-11" :disabled="settingsBusy">
              {{ settingsBusy ? '正在保存…' : '保存设置' }}
            </Button>
            <Button v-if="showSettingsResync" type="button" variant="outline" class="min-h-11" :disabled="settingsBusy" @click="resyncSettings">
              重新同步
            </Button>
          </div>
          <p v-if="settingsAlert" role="alert" class="text-sm text-destructive">
            {{ settingsAlert }}
          </p>
          <p v-if="settingsSuccess" role="status" class="text-sm text-success">
            {{ settingsSuccess }}
          </p>
        </form>
      </CardX>
    </template>
    <Dialog :open="siteGuardOpen" @update:open="value => { if (!value) cancelSiteGuard() }">
      <DialogContent @close-auto-focus="restoreSiteGuardFocus">
        <DialogHeader>
          <DialogTitle>放弃未保存修改？</DialogTitle>
          <DialogDescription v-if="siteGuardPending?.kind === 'refresh'">
            刷新管理数据会丢弃当前站点设置的未保存修改，不会保存任何更改。
          </DialogDescription>
          <DialogDescription v-else>
            离开站点设置栏目会丢弃当前未保存的修改，不会保存任何更改。
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" @click="cancelSiteGuard">
            取消，继续编辑
          </Button>
          <Button variant="destructive" @click="confirmSiteGuard">
            {{ siteGuardPending?.kind === 'refresh' ? '放弃修改并刷新' : '放弃修改并离开' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
