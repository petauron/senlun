import type { InjectionKey } from 'vue'
import type { RouteLocationNormalized } from 'vue-router'
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { onBeforeRouteLeave, onBeforeRouteUpdate } from 'vue-router'
import { decideAdminDeparture, shouldWarnBeforeUnload, unmountLeaveResult } from '@/utils/authFlow'

export interface ActiveSectionDraft {
  section: string
  isDirty: () => boolean
  isBusy: () => boolean
  requestRefresh?: (triggerEl: HTMLElement | null) => boolean
}

export interface AdminDraftRegistry {
  register: (entry: ActiveSectionDraft) => () => void
}

export const ADMIN_DRAFT_REGISTRY_KEY: InjectionKey<AdminDraftRegistry> = Symbol('admin-draft-registry')

export interface PendingLeaveAction {
  kind: 'leave'
  label?: string
}

export interface PendingRefreshAction {
  kind: 'refresh'
  run: () => Promise<void> | void
}

export interface PendingSwitchAction {
  kind: 'switch'
  targetId: string
  targetLabel: string
  apply: () => void
  onConfirmFocus?: () => void
}

export type PendingDraftAction = PendingLeaveAction | PendingRefreshAction | PendingSwitchAction

export interface UseAdminDraftGuardOptions {
  isLoggedIn: () => boolean
  isDirty: () => boolean
  isBusy: () => boolean
  isStayingInSameContext: (to: RouteLocationNormalized) => boolean
  describeRouteTarget?: (to: RouteLocationNormalized) => string | undefined
  registerRouteGuards?: boolean
}

export function useAdminDraftGuard(options: UseAdminDraftGuardOptions) {
  const pending = ref<PendingDraftAction | null>(null)
  const isOpen = computed(() => pending.value !== null)
  let resolveLeave: ((leave: boolean) => void) | null = null
  let discardFocusTarget: HTMLElement | null = null
  let confirmedFocusCallback: (() => void) | null = null
  let beforeUnloadBound = false

  function handleBeforeUnload(event: BeforeUnloadEvent): void {
    if (!shouldWarnBeforeUnload(options.isLoggedIn(), options.isDirty()))
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

  function settleLeave(leave: boolean): void {
    const resolve = resolveLeave
    resolveLeave = null
    if (pending.value?.kind === 'leave')
      pending.value = null
    resolve?.(leave)
  }

  function cancel(): void {
    confirmedFocusCallback = null
    if (pending.value?.kind === 'leave') {
      settleLeave(false)
      return
    }
    pending.value = null
  }

  function confirm(): void {
    const action = pending.value
    if (!action)
      return
    if (action.kind === 'leave') {
      confirmedFocusCallback = null
      settleLeave(true)
      return
    }
    pending.value = null
    if (action.kind === 'switch') {
      if (action.onConfirmFocus) {
        confirmedFocusCallback = action.onConfirmFocus
        discardFocusTarget = null
      }
      else {
        confirmedFocusCallback = null
      }
      action.apply()
      return
    }
    confirmedFocusCallback = null
    void action.run()
  }

  function restoreFocus(event: Event): void {
    event.preventDefault()
    const afterConfirm = confirmedFocusCallback
    confirmedFocusCallback = null
    if (afterConfirm) {
      discardFocusTarget = null
      afterConfirm()
      return
    }
    if (discardFocusTarget?.isConnected)
      discardFocusTarget.focus()
    discardFocusTarget = null
  }

  function captureFocus(triggerEl?: HTMLElement | null): void {
    if (triggerEl) {
      discardFocusTarget = triggerEl
      return
    }
    discardFocusTarget = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null
  }

  function requestSwitch(
    targetId: string,
    targetLabel: string,
    apply: () => void,
    triggerEl?: HTMLElement | null,
    onConfirmFocus?: () => void,
  ): void {
    if (options.isBusy())
      return
    if (!options.isLoggedIn() || !options.isDirty()) {
      apply()
      onConfirmFocus?.()
      return
    }
    cancel()
    captureFocus(triggerEl)
    pending.value = {
      kind: 'switch',
      targetId,
      targetLabel,
      apply,
      onConfirmFocus,
    }
  }

  function requestRefresh(
    runRefresh: () => Promise<void> | void,
    triggerEl?: HTMLElement | null,
  ): boolean {
    if (options.isBusy())
      return true
    if (!options.isLoggedIn() || !options.isDirty())
      return false
    cancel()
    captureFocus(triggerEl)
    pending.value = {
      kind: 'refresh',
      run: runRefresh,
    }
    return true
  }

  function confirmDeparture(to: RouteLocationNormalized): boolean | Promise<boolean> {
    const decision = decideAdminDeparture({
      loggedIn: options.isLoggedIn(),
      stayingInSameContext: options.isStayingInSameContext(to),
      hasUnsavedChanges: options.isDirty(),
      busy: options.isBusy(),
    })
    if (decision === 'allow') {
      if (!options.isLoggedIn()) {
        pending.value = null
        settleLeave(true)
      }
      return true
    }
    if (decision === 'block')
      return false
    cancel()
    captureFocus()
    pending.value = {
      kind: 'leave',
      label: options.describeRouteTarget?.(to),
    }
    return new Promise<boolean>((resolve) => {
      resolveLeave = resolve
    })
  }

  watch(
    () => shouldWarnBeforeUnload(options.isLoggedIn(), options.isDirty()),
    active => syncBeforeUnload(active),
    { immediate: true },
  )

  watch(options.isLoggedIn, (loggedIn) => {
    if (!loggedIn) {
      syncBeforeUnload(false)
      if (pending.value?.kind !== 'leave')
        pending.value = null
      settleLeave(true)
    }
  })

  if (options.registerRouteGuards !== false) {
    onBeforeRouteUpdate(confirmDeparture)
    onBeforeRouteLeave(confirmDeparture)
  }

  onBeforeUnmount(() => {
    syncBeforeUnload(false)
    settleLeave(unmountLeaveResult(options.isLoggedIn()))
  })

  return {
    pending,
    isOpen,
    requestSwitch,
    requestRefresh,
    cancel,
    confirm,
    restoreFocus,
  }
}
