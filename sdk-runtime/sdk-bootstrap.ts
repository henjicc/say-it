(() => {
  const SDK_VERSION = '__SAYIT_AI_SDK_VERSION__'
  const SDK_SOURCE_NAMESPACE = '@henjicc/ai-sdk'
  const CAPABILITY_SOURCES = [
    'bailian-speech-recognition',
    'bailian-speech-recognition-realtime',
    'bailian-translation',
  ] as const
  const GROQ_SOURCE = 'groq-llm' as const
  type CapabilitySource = typeof CAPABILITY_SOURCES[number]
  type ModuleSource = CapabilitySource | typeof GROQ_SOURCE

  interface DisposableRuntime { dispose(): Promise<void> }
  interface CapabilityBundle {
    SAYIT_AI_SDK_SOURCE_NAMESPACE: string
    createSayItCapabilityRuntime(runtime: unknown, options: Record<string, unknown>): DisposableRuntime
  }
  interface GroqBundle {
    createSayItGroqRuntime(runtime: unknown): DisposableRuntime
  }
  interface RuntimeHandle {
    readonly version: string
    readonly sourceNamespace: string
    readonly capabilities?: DisposableRuntime
    readonly groq?: DisposableRuntime
    dispose(): Promise<void>
  }
  interface CreateOptions {
    sources: readonly ModuleSource[]
    capabilityOptions?: Record<string, unknown>
  }

  const target = globalThis as typeof globalThis & {
    __sayitCreateRuntimeContext?: () => unknown
    __sayitAiSdkCapabilities?: CapabilityBundle
    __sayitAiSdkGroq?: GroqBundle
    __sayitCreateSdkRuntime?: (options: CreateOptions) => RuntimeHandle
    __sayitDisposeAllSdkRuntimes?: () => Promise<void>
    __sayitAiSdkManifest?: Readonly<Record<string, unknown>>
  }
  if (typeof target.__sayitCreateRuntimeContext !== 'function') {
    throw new Error('AI SDK bootstrap 缺少 9.12b RuntimeContext adapter')
  }
  if (
    target.__sayitAiSdkCapabilities?.SAYIT_AI_SDK_SOURCE_NAMESPACE
    !== SDK_SOURCE_NAMESPACE
  ) {
    throw new Error('AI SDK capability bundle source namespace 不匹配')
  }
  if (typeof target.__sayitAiSdkGroq?.createSayItGroqRuntime !== 'function') {
    throw new Error('AI SDK Groq bundle 未加载')
  }

  const allowed = new Set<string>([...CAPABILITY_SOURCES, GROQ_SOURCE])
  const active = new Set<RuntimeHandle>()

  target.__sayitAiSdkManifest = Object.freeze({
    version: SDK_VERSION,
    sourceNamespace: SDK_SOURCE_NAMESPACE,
    moduleSources: Object.freeze([...allowed]),
  })

  target.__sayitCreateSdkRuntime = options => {
    const sources = [...new Set(options.sources)]
    for (const source of sources) {
      if (!allowed.has(source)) throw new Error(`未知 AI SDK module source：${source}`)
    }
    const runtime = target.__sayitCreateRuntimeContext!()
    const capabilitySources = sources.filter(
      (source): source is CapabilitySource => source !== GROQ_SOURCE
    )
    const capabilities = capabilitySources.length > 0
      ? target.__sayitAiSdkCapabilities!.createSayItCapabilityRuntime(runtime, {
          ...(options.capabilityOptions ?? {}),
          sources: capabilitySources,
        })
      : undefined
    const groq = sources.includes(GROQ_SOURCE)
      ? target.__sayitAiSdkGroq!.createSayItGroqRuntime(runtime)
      : undefined
    let disposed = false
    const handle: RuntimeHandle = {
      version: SDK_VERSION,
      sourceNamespace: SDK_SOURCE_NAMESPACE,
      capabilities,
      groq,
      dispose: async () => {
        if (disposed) return
        disposed = true
        await capabilities?.dispose()
        await groq?.dispose()
        active.delete(handle)
      },
    }
    active.add(handle)
    return handle
  }

  target.__sayitDisposeAllSdkRuntimes = async () => {
    await Promise.all([...active].map(async runtime => await runtime.dispose()))
    active.clear()
  }
})()
