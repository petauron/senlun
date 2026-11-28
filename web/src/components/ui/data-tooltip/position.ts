export type DataTooltipPlacement = 'top' | 'bottom' | 'left' | 'right'

export interface RectLike {
  top: number
  left: number
  right: number
  bottom: number
  width: number
  height: number
}

export interface SizeLike {
  width: number
  height: number
}

export interface ViewportLike {
  width: number
  height: number
}

export interface TooltipPosition {
  top: number
  left: number
  placement: DataTooltipPlacement
}

export interface TooltipBridgeInsets {
  size: number
  start: number
  end: number
}

const DEFAULT_OFFSET_PX = 8
const DEFAULT_VIEWPORT_PADDING_PX = 8

function computeRawPosition(
  trigger: RectLike,
  tooltip: SizeLike,
  placement: DataTooltipPlacement,
  offset: number,
): { top: number, left: number } {
  switch (placement) {
    case 'top':
      return {
        top: trigger.top - tooltip.height - offset,
        left: trigger.left + (trigger.width - tooltip.width) / 2,
      }
    case 'bottom':
      return {
        top: trigger.bottom + offset,
        left: trigger.left + (trigger.width - tooltip.width) / 2,
      }
    case 'left':
      return {
        top: trigger.top + (trigger.height - tooltip.height) / 2,
        left: trigger.left - tooltip.width - offset,
      }
    case 'right':
      return {
        top: trigger.top + (trigger.height - tooltip.height) / 2,
        left: trigger.right + offset,
      }
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

export function isAnchorOutsideViewport(trigger: RectLike, viewport: ViewportLike): boolean {
  if (trigger.width <= 0 && trigger.height <= 0)
    return true
  return trigger.bottom <= 0
    || trigger.top >= viewport.height
    || trigger.right <= 0
    || trigger.left >= viewport.width
}

export function computeTooltipPosition(
  trigger: RectLike,
  tooltip: SizeLike,
  viewport: ViewportLike,
  preferredPlacement: DataTooltipPlacement,
  offset = DEFAULT_OFFSET_PX,
  padding = DEFAULT_VIEWPORT_PADDING_PX,
): TooltipPosition {
  let placement: DataTooltipPlacement = preferredPlacement
  let raw = computeRawPosition(trigger, tooltip, placement, offset)

  if (placement === 'top' && raw.top < padding && viewport.height - trigger.bottom > trigger.top) {
    placement = 'bottom'
    raw = computeRawPosition(trigger, tooltip, placement, offset)
  }
  else if (placement === 'bottom' && raw.top + tooltip.height > viewport.height - padding && trigger.top > viewport.height - trigger.bottom) {
    placement = 'top'
    raw = computeRawPosition(trigger, tooltip, placement, offset)
  }
  else if (placement === 'left' && raw.left < padding && viewport.width - trigger.right > trigger.left) {
    placement = 'right'
    raw = computeRawPosition(trigger, tooltip, placement, offset)
  }
  else if (placement === 'right' && raw.left + tooltip.width > viewport.width - padding && trigger.left > viewport.width - trigger.right) {
    placement = 'left'
    raw = computeRawPosition(trigger, tooltip, placement, offset)
  }

  const maxLeft = Math.max(padding, viewport.width - tooltip.width - padding)
  const maxTop = Math.max(padding, viewport.height - tooltip.height - padding)

  return {
    top: Math.round(clamp(raw.top, padding, maxTop)),
    left: Math.round(clamp(raw.left, padding, maxLeft)),
    placement,
  }
}

export function computeTooltipBridgeInsets(
  trigger: RectLike,
  tooltip: SizeLike,
  position: TooltipPosition,
): TooltipBridgeInsets {
  if (position.placement === 'top' || position.placement === 'bottom') {
    const size = position.placement === 'bottom'
      ? Math.max(0, Math.floor(position.top - trigger.bottom))
      : Math.max(0, Math.floor(trigger.top - (position.top + tooltip.height)))
    const clampedStart = clamp(trigger.left - position.left, 0, tooltip.width)
    const clampedEnd = clamp(trigger.right - position.left, clampedStart, tooltip.width)
    if (clampedEnd - clampedStart >= 8) {
      return {
        size,
        start: Math.ceil(clampedStart),
        end: Math.ceil(tooltip.width - clampedEnd),
      }
    }
    return {
      size,
      start: 0,
      end: 0,
    }
  }

  const size = position.placement === 'right'
    ? Math.max(0, Math.floor(position.left - trigger.right))
    : Math.max(0, Math.floor(trigger.left - (position.left + tooltip.width)))
  const clampedStart = clamp(trigger.top - position.top, 0, tooltip.height)
  const clampedEnd = clamp(trigger.bottom - position.top, clampedStart, tooltip.height)
  if (clampedEnd - clampedStart >= 8) {
    return {
      size,
      start: Math.ceil(clampedStart),
      end: Math.ceil(tooltip.height - clampedEnd),
    }
  }
  return {
    size,
    start: 0,
    end: 0,
  }
}
