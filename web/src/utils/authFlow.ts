import type { AuthStatus } from '@/stores/auth'

export interface TrafficDepartureContext {
  loggedIn: boolean
  stayingOnTrafficSection: boolean
  hasUnsavedChanges: boolean
  busy: boolean
}

export type TrafficDepartureDecision = 'allow' | 'block' | 'confirm'

export function decideTrafficDeparture(context: TrafficDepartureContext): TrafficDepartureDecision {
  if (!context.loggedIn)
    return 'allow'
  if (context.stayingOnTrafficSection || !context.hasUnsavedChanges)
    return 'allow'
  if (context.busy)
    return 'block'
  return 'confirm'
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
