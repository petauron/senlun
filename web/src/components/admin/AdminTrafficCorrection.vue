<script setup lang="ts">
import type { ManagedNode } from '@/utils/admin'
import type { TrafficCorrectionContext, TrafficUnit } from '@/utils/trafficCorrection'
import { computed, nextTick, ref, watch } from 'vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useAdminAction } from '@/composables/useAdminAction'
import { getSharedApi } from '@/utils/api'
import { billedTraffic, formatTrafficAmount, parseTrafficAmount, trafficDirectionLabels } from '@/utils/trafficCorrection'

const props = defineProps<{ node: ManagedNode, disabled: boolean, refresh: () => Promise<void> }>()
const opened = ref(false)
const context = ref<TrafficCorrectionContext | null>(null)
const unit = ref<TrafficUnit>('gib')
const up = ref('')
const down = ref('')
const reason = ref('')
const preview = ref<{ up: string, down: string, reason: string } | null>(null)
const { busy, error, success, run } = useAdminAction(props.refresh)
const unitLabel = computed(() => unit.value === 'gb' ? 'GB' : 'GiB')
const display = (bytes: string) => `${formatTrafficAmount(bytes, unit.value)} ${unitLabel.value}`
const utc = (ms: number) => `${new Date(ms).toISOString().replace('T', ' ').slice(0, 19)} UTC`
const beforeBilled = computed(() => context.value
  ? billedTraffic(context.value.baseline.used_up_bytes, context.value.baseline.used_down_bytes, context.value.baseline.traffic_limit_type)
  : '0')
const afterBilled = computed(() => context.value && preview.value
  ? billedTraffic(preview.value.up, preview.value.down, context.value.baseline.traffic_limit_type)
  : '0')

watch(() => props.node.id, () => {
  opened.value = false
  context.value = null
  preview.value = null
  error.value = ''
  success.value = ''
})

async function load(reset = false): Promise<void> {
  const id = props.node.id
  context.value = null
  preview.value = null
  await run(async () => {
    const result = await getSharedApi().get<TrafficCorrectionContext>(`admin/nodes/${encodeURIComponent(id)}/traffic-correction`)
    if (props.node.id !== id || !opened.value)
      return
    context.value = result
    if (reset) {
      up.value = formatTrafficAmount(result.baseline.used_up_bytes, unit.value)
      down.value = formatTrafficAmount(result.baseline.used_down_bytes, unit.value)
      reason.value = ''
    }
  }, '', false)
}

async function open(): Promise<void> {
  opened.value = true
  await load(true)
}

function changeUnit(event: Event): void {
  const select = event.target as HTMLSelectElement
  const next = select.value as TrafficUnit
  try {
    const upBytes = parseTrafficAmount(up.value, unit.value)
    const downBytes = parseTrafficAmount(down.value, unit.value)
    unit.value = next
    up.value = formatTrafficAmount(upBytes, next)
    down.value = formatTrafficAmount(downBytes, next)
    error.value = ''
  }
  catch (cause) {
    select.value = unit.value
    error.value = cause instanceof Error ? cause.message : String(cause)
  }
}

function showPreview(): void {
  try {
    if (!reason.value.trim() || new TextEncoder().encode(reason.value.trim()).length > 512)
      throw new Error('请填写校正原因（最多 512 字节）。')
    preview.value = { up: parseTrafficAmount(up.value, unit.value), down: parseTrafficAmount(down.value, unit.value), reason: reason.value.trim() }
    error.value = ''
    void nextTick(() => document.getElementById(`confirm-traffic-${props.node.id}`)?.focus())
  }
  catch (cause) { error.value = cause instanceof Error ? cause.message : String(cause) }
}

async function save(): Promise<void> {
  if (!context.value || !preview.value)
    return
  const baseline = context.value.baseline
  const target = preview.value
  const id = props.node.id
  await run(async () => {
    try {
      await getSharedApi().post(`admin/nodes/${encodeURIComponent(id)}/traffic-correction`, {
        baseline,
        used_up_bytes: target.up,
        used_down_bytes: target.down,
        reason: target.reason,
      })
      opened.value = false
    }
    catch (cause) {
      // A timeout may have committed. Never automatically repeat a write or
      // silently load a new baseline underneath a previously confirmed target.
      context.value = null
      preview.value = null
      throw cause
    }
  }, '已校正本周期本地流量；后续上报继续累加新增流量。')
}
</script>

<template>
  <div class="space-y-2">
    <Button type="button" variant="outline" class="min-h-11" :disabled="disabled || busy || node.disabled" @click="open">
      校正本周期本地流量
    </Button>
    <p v-if="success" role="status" class="text-sm text-success">{{ success }}</p>
    <Dialog :open="opened" @update:open="value => { if (!busy) opened = value }">
      <DialogContent class="max-h-[90dvh] overflow-y-auto sm:max-w-2xl" :show-close="!busy" @escape-key-down="event => { if (busy) event.preventDefault() }" @interact-outside="event => { if (busy) event.preventDefault() }">
        <DialogHeader>
          <DialogTitle>校正本周期本地流量</DialogTitle>
          <DialogDescription>{{ node.name }} · {{ node.id }}。只修改当前周期累计，不修改运营商用量、历史、额度、重置日或 Agent 原始计数。</DialogDescription>
        </DialogHeader>
        <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>
        <p v-if="error && !context" class="text-sm text-muted-foreground">请刷新采样并确认当前值；请求超时时也可能已保存，勿直接重复提交。</p>
        <p v-if="busy" role="status" class="text-sm text-muted-foreground">正在处理…</p>
        <Button type="button" variant="outline" class="min-h-11 w-fit" :disabled="busy" @click="load()">刷新采样与当前值</Button>
        <template v-if="context">
          <div class="space-y-1 break-words rounded-md border p-3 text-sm">
            <p>当前周期开始：{{ utc(context.baseline.cycle_start_ms) }}</p>
            <p>每月 {{ context.baseline.traffic_reset_day }} 日 UTC 重置 · {{ trafficDirectionLabels[context.baseline.traffic_limit_type] }}</p>
            <p>最后采样：{{ utc(context.sampled_at_ms) }}</p>
            <p>上传 {{ display(context.baseline.used_up_bytes) }} / 下载 {{ display(context.baseline.used_down_bytes) }}</p>
            <p>计费已用 {{ display(beforeBilled) }} · 额度 {{ context.traffic_limit_bytes === '0' ? '不限' : display(context.traffic_limit_bytes) }}</p>
          </div>
          <form v-if="!preview" class="space-y-4" :aria-busy="busy" @submit.prevent="showPreview">
            <fieldset :disabled="busy" class="grid gap-4 sm:grid-cols-2">
              <label class="grid gap-2 text-sm sm:col-span-2">输入单位
                <select :value="unit" class="min-h-11 rounded-md border border-input bg-background px-3" @change="changeUnit">
                  <option value="gb">GB（1000³ 字节）</option><option value="gib">GiB（1024³ 字节）</option>
                </select>
              </label>
              <label class="grid gap-2 text-sm">目标上传已用量<Input v-model="up" inputmode="decimal" required maxlength="64" /></label>
              <label class="grid gap-2 text-sm">目标下载已用量<Input v-model="down" inputmode="decimal" required maxlength="64" /></label>
              <label class="grid gap-2 text-sm sm:col-span-2">校正原因<Input v-model="reason" required maxlength="512" /></label>
            </fieldset>
            <p class="text-sm text-muted-foreground">请分别输入上下行，不会把合计同时写入两边。内部保存整数字节，不足 1 字节四舍五入。只采用已保存的计数方式和重置日。</p>
            <Button type="submit" class="min-h-11" :disabled="busy">预览校正</Button>
          </form>
          <div v-else role="region" class="space-y-3 break-words rounded-md border p-3 text-sm" aria-label="本地流量校正预览">
            <p>上传：{{ display(context.baseline.used_up_bytes) }} → {{ display(preview.up) }}</p>
            <p>下载：{{ display(context.baseline.used_down_bytes) }} → {{ display(preview.down) }}</p>
            <p>计费已用：{{ display(beforeBilled) }} → {{ display(afterBilled) }}</p>
            <p>原因：{{ preview.reason }}</p>
            <p class="text-muted-foreground">以上为采样时刻的目标值；确认时仍会保留此采样之后的新增流量。若周期或基准变化，操作将被拒绝，需重新预览。</p>
          </div>
        </template>
        <DialogFooter>
          <Button type="button" variant="outline" :disabled="busy" @click="opened = false">取消</Button>
          <Button v-if="preview" type="button" variant="outline" :disabled="busy" @click="preview = null">返回修改</Button>
          <Button v-if="preview" :id="`confirm-traffic-${node.id}`" type="button" class="min-h-11" :disabled="busy || !context" @click="save">确认校正本地流量</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
