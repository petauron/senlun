import type { AuthStatus } from '@/stores/auth'

export interface AdminDepartureContext {
  loggedIn: boolean
  stayingInSameContext: boolean
  hasUnsavedChanges: boolean
  busy: boolean
}

export type AdminDepartureDecision = 'allow' | 'block' | 'confirm'

export function decideAdminDeparture(context: AdminDepartureContext): AdminDepartureDecision {
  if (!context.loggedIn)
    return 'allow'
  if (context.stayingInSameContext || !context.hasUnsavedChanges)
    return 'allow'
  if (context.busy)
    return 'block'
  return 'confirm'
}

export function shouldWarnBeforeUnload(loggedIn: boolean, hasUnsavedChanges: boolean): boolean {
  return loggedIn && hasUnsavedChanges
}

export interface TrafficDepartureContext {
  loggedIn: boolean
  stayingOnTrafficSection: boolean
  hasUnsavedChanges: boolean
  busy: boolean
}

export type TrafficDepartureDecision = AdminDepartureDecision

export function decideTrafficDeparture(context: TrafficDepartureContext): TrafficDepartureDecision {
  return decideAdminDeparture({
    loggedIn: context.loggedIn,
    stayingInSameContext: context.stayingOnTrafficSection,
    hasUnsavedChanges: context.hasUnsavedChanges,
    busy: context.busy,
  })
}

export function unmountLeaveResult(loggedIn: boolean): boolean {
  return !loggedIn
}

export interface OAuthTotpViewState {
  active: boolean
  expired: boolean
}

export function resolveOAuthTotpViewState(
  oauthTotpQuery: string | null | undefined | readonly (string | null)[],
  status: AuthStatus | null,
  dismissed: boolean,
): OAuthTotpViewState {
  const requested = Array.isArray(oauthTotpQuery)
    ? oauthTotpQuery.includes('required')
    : oauthTotpQuery === 'required'
  if (status === null) {
    return {
      active: !dismissed && requested,
      expired: false,
    }
  }
  const active = !dismissed && status.oauth_totp_required === true
  const expired = requested && status.initialized && !status.logged_in && !status.oauth_totp_required
  return { active, expired }
}
