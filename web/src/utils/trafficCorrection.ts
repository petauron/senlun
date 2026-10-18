export type TrafficUnit = 'gb' | 'gib'
export type TrafficDirection = 'sum' | 'max' | 'min' | 'up' | 'down'

export interface TrafficBaseline {
  sample_id: string
  cycle_start_ms: number
  used_up_bytes: string
  used_down_bytes: string
  audit_cursor: string
  traffic_limit_type: TrafficDirection
  traffic_reset_day: number
}

export interface TrafficCorrectionContext {
  node_id: string
  name: string
  baseline: TrafficBaseline
  sampled_at_ms: number
  traffic_limit_bytes: string
}

const maximumBytes = 9223372036854775807n
const decimalPattern = /^\d+(?:\.\d{1,30})?$/
const factor = (unit: TrafficUnit) => unit === 'gb' ? 1000000000n : 1073741824n

// Do not pass traffic input through Number: accepted counts may exceed 2^53.
export function parseTrafficAmount(raw: string, unit: TrafficUnit): string {
  const value = raw.trim()
  if (value.length > 64 || !decimalPattern.test(value))
    throw new Error('请输入非负十进制流量，最多 30 位小数；不支持科学计数法。')
  const [whole, fraction = ''] = value.split('.')
  const scale = 10n ** BigInt(fraction.length)
  const numerator = (BigInt(whole!) * scale + BigInt(fraction || '0')) * factor(unit)
  const result = (numerator + scale / 2n) / scale
  if (result > maximumBytes)
    throw new Error('流量数值超出可保存范围。')
  return result.toString()
}

export function formatTrafficAmount(bytes: string, unit: TrafficUnit): string {
  const value = BigInt(bytes)
  const divisor = factor(unit)
  const precision = unit === 'gb' ? 9 : 30
  const fraction = ((value % divisor) * 10n ** BigInt(precision) / divisor)
    .toString().padStart(precision, '0').replace(/0+$/, '')
  return `${value / divisor}${fraction ? `.${fraction}` : ''}`
}

export function billedTraffic(up: string, down: string, direction: TrafficDirection): string {
  const a = BigInt(up)
  const b = BigInt(down)
  switch (direction) {
    case 'up': return a.toString()
    case 'down': return b.toString()
    case 'max': return (a > b ? a : b).toString()
    case 'min': return (a < b ? a : b).toString()
    default: return (a + b).toString()
  }
}

export const trafficDirectionLabels: Record<TrafficDirection, string> = {
  sum: '上传 + 下载',
  max: '上传、下载中较大值',
  min: '上传、下载中较小值',
  up: '仅上传',
  down: '仅下载',
}
