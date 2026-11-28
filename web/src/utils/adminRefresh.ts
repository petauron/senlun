export function createSerializedRefresh<T, Options = void>(
  runFetch: (options: Options, isLatest: () => boolean) => Promise<T>,
  onLoadingChange?: (loading: boolean) => void,
) {
  let chain: Promise<void> = Promise.resolve()
  let latestSeq = 0
  let pending = 0
  let latestResult: Promise<T> | null = null

  function refresh(options: Options): Promise<T> {
    const seq = ++latestSeq
    pending += 1
    onLoadingChange?.(true)

    const step = chain.then(async (): Promise<{ applied: boolean, value?: T }> => {
      try {
        if (seq !== latestSeq)
          return { applied: false }
        const value = await runFetch(options, () => seq === latestSeq)
        return {
          applied: seq === latestSeq,
          value,
        }
      }
      finally {
        pending -= 1
        if (pending === 0)
          onLoadingChange?.(false)
      }
    })

    // Advance the serial chain on settle regardless of failure so an older GET error never swallows newer reads.
    chain = step.then(() => undefined, () => undefined)

    // Any caller whose intermediate read was superseded awaits the newer run so it only resolves once fresh data is applied.
    const result = step.then((outcome) => {
      if (outcome.applied || !latestResult)
        return outcome.value as T
      return latestResult
    })
    latestResult = result
    return result
  }

  return {
    refresh,
    isBusy: () => pending > 0,
  }
}
