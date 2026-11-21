import type { IpUnlockReport, UnlockServiceStatus } from '@/utils/rpc'

export const UNLOCK_SERVICE_ORDER = [
  'youtube',
  'netflix',
  'disney_plus',
  'chatgpt',
  'reddit',
  'tiktok',
] as const

const SERVICE_NAMES: Record<string, string> = {
  youtube: 'YouTube',
  netflix: 'Netflix',
  disney_plus: 'Disney+',
  chatgpt: 'ChatGPT',
  reddit: 'Reddit',
  tiktok: 'TikTok',
}

const unlockGreen = 'border-emerald-500/40 bg-emerald-50 text-emerald-700 dark:border-[#228d5a]/50 dark:bg-[#0d2817] dark:text-[#04b84c]'
const unlockAmber = 'border-amber-500/40 bg-amber-50 text-amber-800 dark:border-[#ca8f3e]/50 dark:bg-[#302416] dark:text-[#ca8f3e]'
const unlockRed = 'border-red-500/35 bg-[#fce4ec] text-[#c62828] dark:border-[#f85149]/35 dark:bg-[#2d1416] dark:text-[#f85149]'
const unlockBlue = 'border-blue-500/40 bg-blue-50 text-blue-700 dark:border-[#5796e6]/45 dark:bg-[#102038] dark:text-[#60a5fa]'
const unlockMuted = 'border-border/70 bg-muted/70 text-muted-foreground'

export function getUnlockServiceName(id: string) {
  return SERVICE_NAMES[id] ?? id
}

export function getGoogleUnlockDisplay(report: IpUnlockReport | undefined) {
  if (!report) {
    return {
      label: '未检测',
      shortLabel: 'Google 未检测',
      hint: '尚未检测 Google 地区定位',
      tone: unlockMuted,
      dot: 'bg-muted-foreground',
      isChina: false,
    }
  }
  if (report.address_changed) {
    return {
      label: '地址已变更',
      shortLabel: 'Google 需复测',
      hint: '登记 IP 已变化，需重新检测',
      tone: unlockMuted,
      dot: 'bg-muted-foreground',
      isChina: false,
    }
  }
  const region = report.google.region?.trim() || ''
  if (report.google.status === 'china') {
    return {
      label: '已送中 (CN)',
      shortLabel: 'Google 送中',
      hint: 'Google / YouTube 识别为中国大陆',
      tone: unlockRed,
      dot: 'bg-[#c62828] dark:bg-[#f85149]',
      isChina: true,
    }
  }
  if (report.google.status === 'captcha') {
    return {
      label: region ? `验证受限 (${region})` : '验证受限',
      shortLabel: 'Google 验证',
      hint: 'Google 搜索触发人机验证',
      tone: unlockAmber,
      dot: 'bg-amber-600 dark:bg-[#ca8f3e]',
      isChina: false,
    }
  }
  if (report.google.status === 'normal') {
    return {
      label: region ? `未送中 (${region})` : '未送中',
      shortLabel: region ? `Google ${region}` : 'Google 正常',
      hint: region === 'HK' ? '自然跳转 .com.hk，非 CN' : 'Google / YouTube 定位正常',
      tone: unlockGreen,
      dot: 'bg-emerald-600 dark:bg-[#04b84c]',
      isChina: false,
    }
  }
  return {
    label: '未知',
    shortLabel: 'Google 未知',
    hint: '未能确认 Google 定位状态',
    tone: unlockMuted,
    dot: 'bg-muted-foreground',
    isChina: false,
  }
}

export function getUnlockServiceDisplay(service: UnlockServiceStatus | undefined) {
  if (!service) {
    return {
      label: '未检测',
      tone: unlockMuted,
      title: '尚未检测',
    }
  }
  const region = service.region?.trim() || ''
  switch (service.status) {
    case 'unlocked':
      return {
        label: region ? `解锁 ${region}` : '解锁',
        tone: unlockGreen,
        title: region ? `已解锁 (${region})` : '已解锁',
      }
    case 'originals_only':
      return {
        label: region ? `仅自制 ${region}` : '仅自制剧',
        tone: unlockAmber,
        title: '仅解锁自制剧内容',
      }
    case 'app_only':
      return {
        label: 'App可用',
        tone: unlockBlue,
        title: 'API 通畅，仅匿名网页受 WAF 限制',
      }
    case 'china':
      return {
        label: '送中 CN',
        tone: unlockRed,
        title: '被识别为中国大陆地区',
      }
    case 'blocked':
      return {
        label: region ? `屏蔽 ${region}` : '屏蔽',
        tone: unlockRed,
        title: region ? `当前地区受限 (${region})` : '无法访问或地区屏蔽',
      }
    default:
      return {
        label: '未知',
        tone: unlockMuted,
        title: '本次探测未能确认状态',
      }
  }
}

export function getOrderedUnlockServices(report: IpUnlockReport | undefined) {
  const byId = new Map((report && !report.address_changed ? report.services : []).map(item => [item.id, item]))
  return UNLOCK_SERVICE_ORDER.map((id) => {
    const entry = byId.get(id)
    return {
      id,
      name: getUnlockServiceName(id),
      ...getUnlockServiceDisplay(entry),
    }
  })
}
