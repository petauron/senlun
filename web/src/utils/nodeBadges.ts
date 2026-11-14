import { parseTags } from '@/utils/tagHelper'

export const nodeBadgeClass = 'inline-flex min-h-6 shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-full border px-2 py-1 text-[10px] font-bold leading-none tabular-nums shadow-sm'

export const nodeBadgeTones = {
  unknown: {
    tone: 'border-border/70 bg-muted/70 text-muted-foreground',
  },
  poor: {
    tone: 'border-zinc-400/35 bg-zinc-100 text-zinc-600 dark:border-zinc-500/40 dark:bg-zinc-800/70 dark:text-zinc-400',
  },
  common: {
    tone: 'border-zinc-300/70 bg-white text-zinc-800 dark:border-zinc-400/45 dark:bg-zinc-700/45 dark:text-zinc-100',
  },
  uncommon: {
    tone: 'border-green-500/40 bg-green-50 text-green-800 shadow-green-400/15 dark:bg-green-950/65 dark:text-green-300',
  },
  rare: {
    tone: 'border-blue-500/40 bg-blue-50 text-blue-800 shadow-blue-400/15 dark:bg-blue-950/65 dark:text-blue-300',
  },
  epic: {
    tone: 'border-purple-500/45 bg-purple-50 text-purple-800 shadow-purple-400/20 dark:bg-purple-950/65 dark:text-purple-300',
  },
  legendary: {
    tone: 'border-orange-500/50 bg-linear-to-br from-orange-50 to-orange-100 text-orange-900 shadow-orange-400/25 dark:from-orange-950 dark:to-orange-900/65 dark:text-orange-300',
  },
} as const

export function getIpScoreGrade(score: number | null, stale = false) {
  if (stale || score === null || !Number.isFinite(score) || score < 0 || score > 100) {
    return {
      label: stale ? '待更新' : '待检测',
      tone: nodeBadgeTones.unknown.tone,
    }
  }
  if (score >= 80)
    return { label: '纯净', tone: nodeBadgeTones.uncommon.tone }
  if (score >= 40) {
    return {
      label: '一般',
      tone: 'border-amber-500/40 bg-amber-50 text-amber-800 shadow-amber-400/15 dark:bg-amber-950/65 dark:text-amber-300',
    }
  }
  return {
    label: '高危',
    tone: 'border-red-500/40 bg-red-50 text-red-800 shadow-red-400/15 dark:bg-red-950/65 dark:text-red-300',
  }
}

function parseBandwidthTag(text: string) {
  const match = /^(?:带宽\s*)?(\d+(?:\.\d+)?)\s*([KMG])(?:bps|bit\/s|口)?$/i.exec(text.trim())
  if (!match)
    return null
  const amount = Number(match[1])
  const unit = match[2]!.toUpperCase()
  const megabits = amount * (unit === 'G'
    ? 1000
    : unit === 'K'
      ? 0.001
      : 1)
  if (!Number.isFinite(megabits) || megabits <= 0)
    return null
  const grade = megabits >= 10000
    ? nodeBadgeTones.legendary
    : megabits >= 2500
      ? nodeBadgeTones.epic
      : megabits >= 1000
        ? nodeBadgeTones.rare
        : megabits >= 300
          ? nodeBadgeTones.uncommon
          : megabits >= 100
            ? nodeBadgeTones.common
            : nodeBadgeTones.poor
  return { text: String(amount) + unit, megabits, tone: grade.tone }
}

export function getBandwidthBadge(tags: string | undefined) {
  for (const tag of parseTags(tags)) {
    const bandwidth = parseBandwidthTag(tag.text)
    if (bandwidth)
      return bandwidth
  }
  return null
}

export function getNodeDisplayTags(tags: string | undefined) {
  return parseTags(tags).filter(tag => !parseBandwidthTag(tag.text))
}

export function getReturnRouteGrade(label: string) {
  if (!label.trim() || /未知|未识别|unknown/i.test(label))
    return null
  // These are detected route families, not an inference from latency or speed.
  if (/CN2[-\s]*GIA|CTG[-\s]*GIA|\bGIA\b|\b9929\b|\bCU2\b|\bCMIN2\b/i.test(label))
    return { label: '顶级优化', premium: true, tone: nodeBadgeTones.legendary.tone }
  if (/\b(?:CN2|CN2GT|10099)\b/i.test(label))
    return { label: '一般优化', premium: false, tone: nodeBadgeTones.rare.tone }
  return { label: '普通线路', premium: false, tone: nodeBadgeTones.uncommon.tone }
}
