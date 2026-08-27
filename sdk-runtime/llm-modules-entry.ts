import {
  createLlmModuleClient,
  defineLlmModuleDescriptor,
  type LlmModule,
  type LlmModuleClient,
  type LlmModuleDescriptor,
  type LlmModuleEvent,
  type LlmModuleExecutionContext,
  type LlmModuleExecutionMode,
  type LlmModuleOutput,
  type LlmModuleRequest,
} from '@henjicc/ai-sdk/llm/modules'
import type { DiscoveredModelItem } from '@henjicc/ai-sdk/llm'
import type { RuntimeContext } from '@henjicc/ai-sdk/runtime'

export interface SayItPluginLlmDefinition {
  moduleId: string
  kind: 'llm'
  providerIds: readonly string[]
  modelId: string
  acceptedInputKinds: readonly string[]
  modelDiscovery: boolean
  features: readonly string[]
  tags: readonly string[]
  executionModes: readonly string[]
  contextWindow?: number
  maxOutputTokens?: number
}

export interface SayItPluginLlmRegistryItem {
  pluginId: string
  sourceNamespace: string
  capabilities: readonly SayItPluginLlmDefinition[]
}

export interface SayItPluginLlmProviderAdapter {
  invoke(request: { operation: 'chat' | 'discoverModels'; payload: unknown }): unknown | Promise<unknown>
}

export interface SayItPluginLlmRuntime {
  descriptors(): readonly LlmModuleDescriptor[]
  execute(
    moduleId: string,
    request: LlmModuleRequest,
    options?: {
      requestId?: string
      timeoutMs?: number
      mode?: LlmModuleExecutionMode
      onEvent?(event: LlmModuleEvent): void | Promise<void>
    }
  ): Promise<unknown>
  discover(moduleId: string, options?: { requestId?: string; timeoutMs?: number }): Promise<readonly DiscoveredModelItem[]>
  cancel(requestId: string): void
  drainSource(namespace: string): Promise<number>
  unregisterSource(namespace: string): Promise<number>
  activeRequestCount(): number
  handleProviderEvent(event: unknown): void
  dispose(): Promise<void>
}

interface TerminalState {
  usage?: LlmModuleOutput['usage']
  finishReason?: string | null
  error?: Error
}

interface EventRouter {
  begin(context: LlmModuleExecutionContext): TerminalState
  handle(event: unknown): void
  flush(): Promise<void>
  end(): void
}

export function validateSayItPluginLlmRegistry(
  registryJson: string,
  builtinDescriptorsJson: string
): string {
  const registry = parseRegistry(registryJson)
  const client = createLlmModuleClient({ runtime: validationRuntime() })
  const builtinDescriptors: unknown = JSON.parse(builtinDescriptorsJson)
  if (!Array.isArray(builtinDescriptors)) throw new Error('内置 LLM descriptors 必须是数组')
  for (const descriptor of builtinDescriptors) {
    client.register(validationModule(defineLlmModuleDescriptor(descriptor as LlmModuleDescriptor)))
  }
  for (const plugin of registry) {
    if (plugin.sourceNamespace !== plugin.pluginId) {
      throw new Error(`插件 ${plugin.pluginId} 的 source namespace 必须由宿主固定为自身 ID`)
    }
    for (const definition of plugin.capabilities) {
      try {
        client.register(validationModule(pluginDescriptor(plugin.sourceNamespace, definition)))
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error)
        throw new Error(`插件 LLM module ${definition.moduleId} 注册失败：${message}`)
      }
    }
  }
  const descriptors = client.list().filter(descriptor => descriptor.source.kind === 'plugin')
  void client.dispose()
  return JSON.stringify(descriptors)
}

export function createSayItPluginLlmRuntime(
  runtime: RuntimeContext,
  sourceNamespace: string,
  definitions: readonly SayItPluginLlmDefinition[],
  provider: SayItPluginLlmProviderAdapter
): SayItPluginLlmRuntime {
  const client = createLlmModuleClient({ runtime })
  const router = createEventRouter()
  const active = new Set<string>()
  for (const definition of definitions) {
    client.register(pluginModule(pluginDescriptor(sourceNamespace, definition), definition, provider, router))
  }
  let disposed = false
  const ensureActive = (): void => {
    if (disposed) throw new Error(`插件 LLM runtime ${sourceNamespace} 已销毁`)
  }
  return {
    descriptors: () => {
      ensureActive()
      return client.list()
    },
    execute: async (moduleId, request, options = {}) => {
      ensureActive()
      const requestId = options.requestId ?? cryptoSafeRequestId()
      active.add(requestId)
      try {
        return await client.execute(moduleId, request, { ...options, requestId })
      } finally {
        active.delete(requestId)
      }
    },
    discover: async (moduleId, options = {}) => {
      ensureActive()
      const requestId = options.requestId ?? cryptoSafeRequestId()
      active.add(requestId)
      try {
        return await client.discover(moduleId, { ...options, requestId })
      } finally {
        active.delete(requestId)
      }
    },
    cancel: requestId => client.cancel(requestId),
    drainSource: async namespace => await client.drainSource(namespace),
    unregisterSource: async namespace => await client.unregisterSource(namespace),
    activeRequestCount: () => active.size,
    handleProviderEvent: event => router.handle(event),
    dispose: async () => {
      if (disposed) return
      await client.unregisterSource(sourceNamespace)
      disposed = true
      await client.dispose()
      active.clear()
    },
  }
}

function pluginDescriptor(
  sourceNamespace: string,
  definition: SayItPluginLlmDefinition
): LlmModuleDescriptor {
  if (definition.kind !== 'llm') throw new Error(`LLM adapter 收到非 llm module：${definition.moduleId}`)
  const inputKinds = new Set(definition.acceptedInputKinds)
  if (!inputKinds.has('text')) throw new Error(`LLM module ${definition.moduleId} 必须接受 text 输入`)
  for (const kind of inputKinds) {
    if (!['text', 'image', 'audio', 'video'].includes(kind)) {
      throw new Error(`LLM module ${definition.moduleId} 使用未知输入类型：${kind}`)
    }
  }
  const modes = definition.executionModes.map(mode => {
    if (mode !== 'request-response' && mode !== 'event-stream') {
      throw new Error(`LLM module ${definition.moduleId} 使用未知 executionMode：${mode}`)
    }
    return mode
  })
  if (modes.length === 0) throw new Error(`LLM module ${definition.moduleId} 缺少 executionModes`)
  const feature = (name: string): boolean => definition.features.includes(name)
  return defineLlmModuleDescriptor({
    id: definition.moduleId,
    source: { kind: 'plugin', namespace: sourceNamespace },
    providerId: singleProviderId(definition),
    modelId: definition.modelId,
    capabilities: {
      text: true,
      image: inputKinds.has('image'),
      audio: inputKinds.has('audio'),
      video: inputKinds.has('video'),
      streaming: modes.includes('event-stream'),
      toolCall: feature('tool-call'),
      parallelTools: feature('parallel-tools'),
      jsonOutput: feature('json-output') || feature('structured-schema'),
      structuredOutputMode: feature('structured-schema') ? 'schema' : feature('json-output') ? 'json' : 'none',
      reasoning: feature('reasoning'),
      sampling: feature('sampling'),
      contextWindow: definition.contextWindow ?? null,
      maxOutputTokens: definition.maxOutputTokens ?? null,
      usage: feature('usage'),
    },
    executionModes: modes,
    tags: definition.tags,
  })
}

function pluginModule(
  descriptor: LlmModuleDescriptor,
  definition: SayItPluginLlmDefinition,
  provider: SayItPluginLlmProviderAdapter,
  router: EventRouter
): LlmModule {
  return {
    descriptor,
    execute: async (request, context) => {
      const terminal = router.begin(context)
      try {
        const raw = await raceAbort(
          Promise.resolve(provider.invoke({
            operation: 'chat',
            payload: { ...request, requestId: context.requestId, mode: context.mode },
          })),
          context.signal
        )
        await router.flush()
        if (terminal.error) throw terminal.error
        const output = normalizeOutput(raw)
        return {
          ...output,
          usage: terminal.usage ?? output.usage,
          finishReason: terminal.finishReason ?? output.finishReason,
        }
      } finally {
        router.end()
      }
    },
    discover: definition.modelDiscovery
      ? async context => normalizeDiscoveredModels(await raceAbort(
          Promise.resolve(provider.invoke({
            operation: 'discoverModels',
            payload: {
              providerId: descriptor.providerId,
              modelId: descriptor.modelId,
              requestId: context.requestId,
            },
          })),
          context.signal
        ))
      : undefined,
  }
}

function createEventRouter(): EventRouter {
  let context: LlmModuleExecutionContext | undefined
  let terminal: TerminalState | undefined
  let pending = Promise.resolve()
  return {
    begin: next => {
      if (context) throw new Error('同一插件 LLM runtime 不允许并发复用事件通道')
      context = next
      terminal = {}
      pending = Promise.resolve()
      return terminal
    },
    handle: event => {
      if (!context || !terminal || !isRecord(event) || typeof event.type !== 'string') return
      if (event.type === 'text' && typeof event.text === 'string') {
        const text = event.text
        pending = pending.then(async () => await context!.emit({ type: 'Token', data: text }))
      } else if (event.type === 'reasoning' && typeof event.text === 'string') {
        const text = event.text
        pending = pending.then(async () => await context!.emit({ type: 'ReasoningToken', data: text }))
      } else if (event.type === 'usage') {
        terminal.usage = normalizeUsage(event.data ?? event.usage)
      } else if (event.type === 'finish') {
        terminal.finishReason = typeof event.finishReason === 'string' ? event.finishReason : null
      } else if (event.type === 'error') {
        terminal.error = new Error(typeof event.message === 'string' ? event.message : '插件 LLM 执行失败')
      }
    },
    flush: async () => await pending,
    end: () => {
      context = undefined
      terminal = undefined
      pending = Promise.resolve()
    },
  }
}

function validationModule(descriptor: LlmModuleDescriptor): LlmModule {
  return {
    descriptor,
    execute: async () => ({ output: '', reasoningOutput: '', usage: null, finishReason: null }),
  }
}

function normalizeOutput(value: unknown): LlmModuleOutput {
  if (!isRecord(value)) throw new Error('插件 chat 必须返回对象')
  return {
    output: typeof value.output === 'string' ? value.output : '',
    reasoningOutput: typeof value.reasoningOutput === 'string' ? value.reasoningOutput : '',
    usage: normalizeUsage(value.usage),
    finishReason: typeof value.finishReason === 'string' ? value.finishReason : null,
  }
}

function normalizeUsage(value: unknown): LlmModuleOutput['usage'] {
  if (!isRecord(value)) return null
  const token = (key: string): number | null => {
    const raw = value[key]
    return typeof raw === 'number' && Number.isFinite(raw) && raw >= 0 ? raw : null
  }
  return {
    inputTokens: token('inputTokens'), outputTokens: token('outputTokens'),
    reasoningTokens: token('reasoningTokens'), cacheReadTokens: token('cacheReadTokens'),
    cacheWriteTokens: token('cacheWriteTokens'), totalTokens: token('totalTokens'),
  }
}

function normalizeDiscoveredModels(value: unknown): DiscoveredModelItem[] {
  if (!Array.isArray(value)) throw new Error('插件 discoverModels 必须返回数组')
  return value.map((item, index) => {
    if (!isRecord(item) || typeof item.modelId !== 'string' || !item.modelId.trim()) {
      throw new Error(`插件 discoverModels 第 ${index + 1} 项缺少 modelId`)
    }
    return {
      modelId: item.modelId.trim(),
      displayName: typeof item.displayName === 'string' && item.displayName.trim()
        ? item.displayName.trim()
        : item.modelId.trim(),
      contextWindow: finitePositiveOrNull(item.contextWindow),
      maxOutputTokens: finitePositiveOrNull(item.maxOutputTokens),
    }
  })
}

function raceAbort<T>(operation: Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted) return Promise.reject(signal.reason ?? new Error('插件 LLM 操作已取消'))
  return new Promise<T>((resolve, reject) => {
    const abort = (): void => reject(signal.reason ?? new Error('插件 LLM 操作已取消'))
    signal.addEventListener('abort', abort, { once: true })
    operation.then(resolve, reject).finally(() => signal.removeEventListener('abort', abort))
  })
}

function singleProviderId(definition: SayItPluginLlmDefinition): string {
  if (definition.providerIds.length !== 1 || !definition.providerIds[0]?.trim()) {
    throw new Error(`LLM module ${definition.moduleId} 必须声明唯一 providerId`)
  }
  return definition.providerIds[0]
}

function parseRegistry(json: string): readonly SayItPluginLlmRegistryItem[] {
  const value: unknown = JSON.parse(json)
  if (!Array.isArray(value)) throw new Error('插件 LLM registry 必须是数组')
  return value as SayItPluginLlmRegistryItem[]
}

function finitePositiveOrNull(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : null
}

function cryptoSafeRequestId(): string {
  return `plugin-llm-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

function validationRuntime(): RuntimeContext {
  return {
    transport: { fetch: async () => { throw new Error('manifest validation 禁止网络') } },
    credentials: { get: async () => undefined },
    media: { read: async () => { throw new Error('manifest validation 禁止媒体读取') } },
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}
