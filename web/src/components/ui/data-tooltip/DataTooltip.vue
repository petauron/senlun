<script setup lang="ts">
import type { CSSProperties, HTMLAttributes } from 'vue'
import type { DataTooltipPlacement } from './position'
import { computed, onBeforeUnmount, ref, useId, useSlots, watch } from 'vue'
import { cn } from '@/lib/utils'
import { computeTooltipPosition } from './position'

interface Props {
  /** 提示文本，留空且无 #content 插槽时不渲染气泡 */
  content?: string
  /** 气泡相对触发元素的方位 */
  placement?: DataTooltipPlacement
  /** 气泡宽度，number 视为 px；默认由内容撑起 */
  width?: number | string
  /** 气泡高度，number 视为 px；默认由内容撑起 */
  height?: number | string
  /** 包裹元素标签，默认 div */
  as?: string
  /** 包裹元素的附加类 */
  class?: HTMLAttributes['class']
  /** 气泡的附加类 */
  contentClass?: HTMLAttributes['class']
  /** Let informational tooltips inside navigable cards preserve card clicks. */
  stopClickPropagation?: boolean
  /** 显式禁用气泡渲染与关联 */
  disabled?: boolean
  /** 将 aria-describedby 绑定到包裹元素还是通过默认插槽显式绑定到真实焦点控件 */
  describeTarget?: 'root' | 'slot'
  /** 为可聚焦控件常驻隐藏描述节点，确保获焦瞬间即可被辅助技术读取 */
  persistDescription?: boolean
  /** 保留调用方已有的 aria-describedby 关联 */
  ariaDescribedby?: string
}

const props = withDefaults(defineProps<Props>(), {
  placement: 'top',
  as: 'div',
  stopClickPropagation: true,
  disabled: false,
  describeTarget: 'root',
  persistDescription: false,
})
defineSlots<{
  default?: (props: { describedBy: string | undefined }) => unknown
  content?: () => unknown
}>()
const slots = useSlots()

const HOVER_CLOSE_DELAY_MS = 120

const tooltipId = useId()
const rootRef = ref<HTMLElement | null>(null)
const tooltipRef = ref<HTMLElement | null>(null)
const isTouchOpen = ref(false)
const isHoverOpen = ref(false)
const isFocusOpen = ref(false)
const tooltipPosition = ref<{ top: number, left: number, placement: DataTooltipPlacement }>({
  top: 0,
  left: 0,
  placement: props.placement,
})

let lastTouchOpenAt = 0
let shouldStopNextClick = false
let closeHoverTimer: ReturnType<typeof setTimeout> | null = null
let positionFrame: number | null = null
let isDisposed = false

const hasTooltip = computed(() => !props.disabled && Boolean(props.content || slots.content))
const isVisible = computed(() => hasTooltip.value && (isTouchOpen.value || isHoverOpen.value || isFocusOpen.value))
const activeDescribedBy = computed(() => {
  if (!hasTooltip.value)
    return undefined
  return props.persistDescription || isVisible.value
    ? tooltipId
    : undefined
})
const rootDescribedBy = computed(() => {
  const caller = props.ariaDescribedby?.trim() ?? ''
  const own = props.describeTarget === 'root'
    ? activeDescribedBy.value ?? ''
    : ''
  const merged = [caller, own].filter(Boolean).join(' ')
  return merged || undefined
})

const tooltipStyle = computed<CSSProperties>(() => {
  const style: CSSProperties = {
    top: `${tooltipPosition.value.top}px`,
    left: `${tooltipPosition.value.left}px`,
  }
  if (props.width != null) {
    style.width = typeof props.width === 'number'
      ? `${props.width}px`
      : props.width
  }
  if (props.height != null) {
    style.height = typeof props.height === 'number'
      ? `${props.height}px`
      : props.height
  }
  return style
})

function isTouchLikePointer(event: PointerEvent) {
  const hasCoarsePointer = typeof window !== 'undefined'
    && typeof window.matchMedia === 'function'
    && window.matchMedia('(hover: none), (pointer: coarse)').matches

  return event.pointerType !== 'mouse' || hasCoarsePointer
}

function cancelScheduledHoverClose() {
  if (closeHoverTimer !== null) {
    clearTimeout(closeHoverTimer)
    closeHoverTimer = null
  }
}

function cancelScheduledPositionUpdate() {
  if (typeof window !== 'undefined' && positionFrame !== null) {
    window.cancelAnimationFrame(positionFrame)
    positionFrame = null
  }
}

function scheduleHoverClose() {
  cancelScheduledHoverClose()
  closeHoverTimer = setTimeout(() => {
    closeHoverTimer = null
    isHoverOpen.value = false
  }, HOVER_CLOSE_DELAY_MS)
}

function updatePosition() {
  const root = rootRef.value
  const tooltip = tooltipRef.value
  if (isDisposed || !isVisible.value || !root || !tooltip || typeof window === 'undefined')
    return

  const triggerRect = root.getBoundingClientRect()
  const tooltipRect = tooltip.getBoundingClientRect()
  tooltipPosition.value = computeTooltipPosition(
    triggerRect,
    {
      width: tooltipRect.width,
      height: tooltipRect.height,
    },
    {
      width: window.innerWidth,
      height: window.innerHeight,
    },
    props.placement,
  )
}

function scheduleFramePositionUpdate() {
  if (typeof window === 'undefined' || isDisposed || !isVisible.value || positionFrame !== null)
    return
  positionFrame = window.requestAnimationFrame(() => {
    positionFrame = null
    updatePosition()
  })
}

function closeTooltip() {
  cancelScheduledHoverClose()
  isTouchOpen.value = false
  isHoverOpen.value = false
  isFocusOpen.value = false
}

function removeActiveListeners() {
  cancelScheduledHoverClose()
  cancelScheduledPositionUpdate()
  if (typeof document !== 'undefined') {
    document.removeEventListener('pointerdown', handleDocumentPointerDown, true)
    document.removeEventListener('keydown', handleDocumentKeydown)
  }
  if (typeof window !== 'undefined') {
    window.removeEventListener('scroll', scheduleFramePositionUpdate, true)
    window.removeEventListener('resize', scheduleFramePositionUpdate)
  }
}

function handleDocumentPointerDown(event: PointerEvent) {
  const target = event.target as Node | null
  if (!target || rootRef.value?.contains(target) || tooltipRef.value?.contains(target))
    return

  closeTooltip()
}

function handleDocumentKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape')
    closeTooltip()
}

function handlePointerDown(event: PointerEvent) {
  if (!hasTooltip.value || !isTouchLikePointer(event))
    return

  lastTouchOpenAt = Date.now()
  shouldStopNextClick = true
  cancelScheduledHoverClose()
  isTouchOpen.value = !isTouchOpen.value
  isHoverOpen.value = false
  isFocusOpen.value = false
}

function handleClick(event: MouseEvent) {
  if (props.stopClickPropagation && shouldStopNextClick && Date.now() - lastTouchOpenAt < 800)
    event.stopPropagation()

  shouldStopNextClick = false
}

function handleTriggerPointerEnter(event: PointerEvent) {
  if (!hasTooltip.value || event.pointerType === 'touch')
    return
  cancelScheduledHoverClose()
  isHoverOpen.value = true
}

function handleTriggerPointerLeave(event: PointerEvent) {
  if (event.pointerType === 'touch')
    return
  scheduleHoverClose()
}

function handleTooltipPointerEnter() {
  if (!hasTooltip.value)
    return
  cancelScheduledHoverClose()
  isHoverOpen.value = true
}

function handleTooltipPointerLeave(event: PointerEvent) {
  if (event.pointerType === 'touch')
    return
  scheduleHoverClose()
}

function handleFocusIn() {
  if (!hasTooltip.value)
    return
  cancelScheduledHoverClose()
  isFocusOpen.value = true
}

function handleFocusOut(event: FocusEvent) {
  const related = event.relatedTarget as Node | null
  if (related && (rootRef.value?.contains(related) || tooltipRef.value?.contains(related)))
    return
  isFocusOpen.value = false
}

watch(isVisible, (visible) => {
  if (typeof document === 'undefined' || typeof window === 'undefined')
    return

  if (visible) {
    document.addEventListener('pointerdown', handleDocumentPointerDown, true)
    document.addEventListener('keydown', handleDocumentKeydown)
    window.addEventListener('scroll', scheduleFramePositionUpdate, true)
    window.addEventListener('resize', scheduleFramePositionUpdate)
    updatePosition()
    return
  }

  removeActiveListeners()
}, { flush: 'post' })

watch(
  () => [props.content, props.placement, props.width, props.height],
  () => {
    if (isVisible.value)
      scheduleFramePositionUpdate()
  },
)

watch(hasTooltip, (value) => {
  if (!value)
    closeTooltip()
})

onBeforeUnmount(() => {
  isDisposed = true
  removeActiveListeners()
})
</script>

<template>
  <component
    :is="as"
    ref="rootRef"
    data-slot="data-tooltip"
    :data-state="isVisible ? 'open' : 'closed'"
    :aria-describedby="rootDescribedBy"
    :class="cn('group/data-tooltip relative inline-block', props.class)"
    @pointerdown.capture="handlePointerDown"
    @pointerenter="handleTriggerPointerEnter"
    @pointerleave="handleTriggerPointerLeave"
    @focusin="handleFocusIn"
    @focusout="handleFocusOut"
    @click="handleClick"
  >
    <slot :described-by="props.describeTarget === 'slot' ? activeDescribedBy : undefined" />
    <Teleport to="body">
      <span
        v-if="hasTooltip && (persistDescription || isVisible)"
        v-show="isVisible"
        :id="tooltipId"
        ref="tooltipRef"
        role="tooltip"
        data-slot="data-tooltip-content"
        :data-placement="tooltipPosition.placement"
        :class="cn(
          'data-tooltip-content fixed z-40 w-max max-w-[calc(100vw-1rem)] rounded bg-foreground/90 px-1.5 py-1 text-[10px] leading-normal text-background shadow-lg whitespace-normal break-words',
          props.contentClass,
        )"
        :style="tooltipStyle"
        @pointerenter="handleTooltipPointerEnter"
        @pointerleave="handleTooltipPointerLeave"
        @click.stop
      >
        <slot name="content">{{ content }}</slot>
      </span>
    </Teleport>
  </component>
</template>

<style scoped>
.data-tooltip-content::before {
  content: '';
  position: absolute;
}

.data-tooltip-content[data-placement='top']::before {
  left: 0;
  right: 0;
  top: 100%;
  height: 10px;
}

.data-tooltip-content[data-placement='bottom']::before {
  left: 0;
  right: 0;
  bottom: 100%;
  height: 10px;
}

.data-tooltip-content[data-placement='left']::before {
  top: 0;
  bottom: 0;
  left: 100%;
  width: 10px;
}

.data-tooltip-content[data-placement='right']::before {
  top: 0;
  bottom: 0;
  right: 100%;
  width: 10px;
}
</style>
