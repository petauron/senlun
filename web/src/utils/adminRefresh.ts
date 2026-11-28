export function createSerializedRefresh<T, Options = void>(
  runFetch: (options: Options, isLatest: () => boolean) => Promise<T>,
  onLoadingChange?: (loading: boolean) => void,
) {
  let inFlight: Promise<T> | null = null
  let queued = 0

  const isLatest = () => queued === 0

  async function refresh(options: Options): Promise<T> {
    if (inFlight) {
      queued++
      try {
        while (inFlight)
          await inFlight.catch(() => {})
      }
      finally {
        queued--
      }
    }

    onLoadingChange?.(true)
    const current = (async () => {
      try {
        return await runFetch(options, isLatest)
      }
      finally {
        if (inFlight === current)
          inFlight = null
        if (!inFlight && queued === 0)
          onLoadingChange?.(false)
      }
    })()
    inFlight = current
    return current
  }

  return {
    refresh,
    isBusy: () => inFlight !== null || queued > 0,
  }
}
