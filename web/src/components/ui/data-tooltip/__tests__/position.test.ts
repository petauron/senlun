import { describe, expect, it } from 'vitest'
import { computeTooltipPosition, isAnchorOutsideViewport } from '../position'

describe('computeTooltipPosition', () => {
  const viewport = {
    width: 1280,
    height: 720,
  }
  const tooltip = {
    width: 160,
    height: 36,
  }

  it('centers top placement above the trigger with default offset', () => {
    const trigger = {
      top: 200,
      left: 500,
      right: 580,
      bottom: 224,
      width: 80,
      height: 24,
    }

    expect(computeTooltipPosition(trigger, tooltip, viewport, 'top')).toEqual({
      top: 156,
      left: 460,
      placement: 'top',
    })
  })

  it('flips top placement to bottom when clipped by the top edge', () => {
    const trigger = {
      top: 16,
      left: 500,
      right: 580,
      bottom: 40,
      width: 80,
      height: 24,
    }

    expect(computeTooltipPosition(trigger, tooltip, viewport, 'top')).toEqual({
      top: 48,
      left: 460,
      placement: 'bottom',
    })
  })

  it('flips left placement to right when near the left viewport edge', () => {
    const trigger = {
      top: 300,
      left: 12,
      right: 52,
      bottom: 324,
      width: 40,
      height: 24,
    }

    expect(computeTooltipPosition(trigger, tooltip, viewport, 'left')).toEqual({
      top: 294,
      left: 60,
      placement: 'right',
    })
  })

  it('clamps horizontally inside narrow mobile viewports', () => {
    const mobileViewport = {
      width: 390,
      height: 844,
    }
    const wideTooltip = {
      width: 240,
      height: 40,
    }
    const trigger = {
      top: 400,
      left: 310,
      right: 378,
      bottom: 424,
      width: 68,
      height: 24,
    }

    expect(computeTooltipPosition(trigger, wideTooltip, mobileViewport, 'top')).toEqual({
      top: 352,
      left: 142,
      placement: 'top',
    })
  })
})

describe('isAnchorOutsideViewport', () => {
  const viewport = {
    width: 1280,
    height: 720,
  }

  it('returns false when the trigger overlaps the viewport', () => {
    expect(isAnchorOutsideViewport({
      top: -10,
      left: 100,
      right: 180,
      bottom: 14,
      width: 80,
      height: 24,
    }, viewport)).toBe(false)
  })

  it('returns true when the trigger is scrolled completely above or past the viewport', () => {
    expect(isAnchorOutsideViewport({
      top: -40,
      left: 100,
      right: 180,
      bottom: 0,
      width: 80,
      height: 40,
    }, viewport)).toBe(true)

    expect(isAnchorOutsideViewport({
      top: 720,
      left: 100,
      right: 180,
      bottom: 744,
      width: 80,
      height: 24,
    }, viewport)).toBe(true)

    expect(isAnchorOutsideViewport({
      top: 100,
      left: 1280,
      right: 1320,
      bottom: 124,
      width: 40,
      height: 24,
    }, viewport)).toBe(true)
  })
})
