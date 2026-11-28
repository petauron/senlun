import type { ManagedNode, PeerProbeSettings, SiteSettings } from './admin'
import { nodeMetadata } from './admin'

export type NodeDraftForm = ManagedNode & { peer_probe: PeerProbeSettings }

export interface NodeDraftSnapshot {
  draft: NodeDraftForm
  expires: string
  quotaGiB: number
}

export interface NodeDraftCurrentState {
  draft: NodeDraftForm | null
  expires: string
  quotaGiB: number | string
  newTagText: string
}

export interface NodeServerSyncContext {
  incomingNodeId: string | null
  loadedNodeId: string | null
  hasUnsavedChanges: boolean
  forceReset: boolean
}

const DEFAULT_PEER_PROBE: PeerProbeSettings = {
  kind: 'tcp',
  family: 'auto',
  disabled: false,
}

export function formatLocalDateTimeInput(unixMs: number | null | undefined): string {
  if (!unixMs)
    return ''
  const date = new Date(unixMs)
  return new Date(date.getTime() - date.getTimezoneOffset() * 60000).toISOString().slice(0, 16)
}

export function cloneNodeDraft(draft: NodeDraftForm): NodeDraftForm {
  return {
    ...draft,
    peer_probe: {
      ...draft.peer_probe,
    },
  }
}

export function createNodeDraftSnapshot(node: ManagedNode): NodeDraftSnapshot {
  return {
    draft: {
      ...nodeMetadata(node),
      ipv4: node.ipv4 ?? '',
      ipv6: node.ipv6 ?? '',
      traffic_source: node.traffic_source ?? 'local',
      peer_probe: {
        ...(node.peer_probe ?? DEFAULT_PEER_PROBE),
      },
    },
    expires: formatLocalDateTimeInput(node.expired_at_unix_ms),
    quotaGiB: (node.traffic_limit_bytes ?? 0) / 1024 ** 3,
  }
}

function sameText(a: string | undefined, b: string | undefined): boolean {
  return (a ?? '') === (b ?? '')
}

function sameNumericValue(a: number | string | undefined, b: number | string | undefined): boolean {
  const sa = String(a ?? '').trim()
  const sb = String(b ?? '').trim()
  if (sa === sb)
    return true
  if (!sa || !sb)
    return false
  const na = Number(sa)
  const nb = Number(sb)
  return Number.isFinite(na) && Number.isFinite(nb) && na === nb
}

export function isNodeDraftDirty(
  current: NodeDraftCurrentState,
  baseline: NodeDraftSnapshot | null,
): boolean {
  if (!current.draft || !baseline)
    return false
  if (current.newTagText.trim().length > 0)
    return true
  if (current.expires !== baseline.expires)
    return true
  if (!sameNumericValue(current.quotaGiB, baseline.quotaGiB))
    return true

  const a = current.draft
  const b = baseline.draft
  return !(
    sameText(a.name, b.name)
    && sameText(a.region, b.region)
    && sameText(a.ipv4, b.ipv4)
    && sameText(a.ipv6, b.ipv6)
    && sameText(a.group, b.group)
    && sameNumericValue(a.weight, b.weight)
    && sameText(a.tags, b.tags)
    && a.hidden === b.hidden
    && sameText(a.public_remark, b.public_remark)
    && sameNumericValue(a.price, b.price)
    && sameText(a.currency, b.currency)
    && sameNumericValue(a.billing_cycle_days, b.billing_cycle_days)
    && a.auto_renewal === b.auto_renewal
    && (a.traffic_source ?? 'local') === (b.traffic_source ?? 'local')
    && a.traffic_limit_type === b.traffic_limit_type
    && sameNumericValue(a.traffic_reset_day, b.traffic_reset_day)
    && a.peer_probe.kind === b.peer_probe.kind
    && a.peer_probe.family === b.peer_probe.family
    && a.peer_probe.disabled === b.peer_probe.disabled
  )
}

export function shouldSyncNodeDraftFromServer(context: NodeServerSyncContext): boolean {
  if (!context.incomingNodeId)
    return true
  if (context.forceReset)
    return true
  if (context.incomingNodeId !== context.loadedNodeId)
    return true
  return !context.hasUnsavedChanges
}

export function normalizeSiteSettings(settings: SiteSettings): SiteSettings {
  const mode = ['earth', 'earth-stop'].includes(settings.earth_view_mode)
    ? 'maps'
    : settings.earth_view_mode
  return {
    site_name: settings.site_name,
    private_site: Boolean(settings.private_site),
    agent_interval_seconds: Number(settings.agent_interval_seconds),
    peer_probe_interval_seconds: Number(settings.peer_probe_interval_seconds),
    ip_info_enabled: Boolean(settings.ip_info_enabled),
    daily_exchange_rates: Boolean(settings.daily_exchange_rates),
    earth_view_mode: mode,
  }
}

export function isSiteSettingsDirty(current: SiteSettings, baseline: SiteSettings | null): boolean {
  if (!baseline)
    return false
  return !(
    sameText(current.site_name, baseline.site_name)
    && Boolean(current.private_site) === Boolean(baseline.private_site)
    && sameNumericValue(current.agent_interval_seconds, baseline.agent_interval_seconds)
    && sameNumericValue(current.peer_probe_interval_seconds, baseline.peer_probe_interval_seconds)
    && Boolean(current.ip_info_enabled) === Boolean(baseline.ip_info_enabled)
    && Boolean(current.daily_exchange_rates) === Boolean(baseline.daily_exchange_rates)
    && current.earth_view_mode === baseline.earth_view_mode
  )
}
