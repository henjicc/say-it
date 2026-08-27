import {
  createCapabilityClient,
  type CapabilityClient,
  type CapabilityDescriptor,
  type CapabilityExecutionMode,
  type CapabilityExecuteOptions,
  type CapabilityModule,
  type CapabilityRealtimeModule,
  type CapabilityRealtimeSession,
} from '@henjicc/ai-sdk/capabilities'
import {
  bailianNonRealtimeAsrPresets,
  createBailianAsrModule,
  type BailianAsrModuleOptions,
} from '@henjicc/ai-sdk/capabilities/speech-recognition/bailian'
import {
  bailianRealtimeAsrPresets,
  createBailianRealtimeAsrModule,
  type BailianRealtimeModuleOptions,
} from '@henjicc/ai-sdk/capabilities/speech-recognition/bailian/realtime'
import {
  BAILIAN_QWEN_MT_PRESETS,
  createBailianQwenMtTranslationModule,
  type BailianQwenMtModuleConfig,
} from '@henjicc/ai-sdk/capabilities/translation/bailian'
import type { RuntimeContext } from '@henjicc/ai-sdk/runtime'

export const SAYIT_AI_SDK_SOURCE_NAMESPACE = '@henjicc/ai-sdk'

export const SAYIT_CAPABILITY_MODULE_SOURCES = [
  'bailian-speech-recognition',
  'bailian-speech-recognition-realtime',
  'bailian-translation',
] as const

export type SayItCapabilityModuleSource = typeof SAYIT_CAPABILITY_MODULE_SOURCES[number]

export interface SayItCapabilityRuntimeOptions {
  sources: readonly SayItCapabilityModuleSource[]
  bailianSpeechRecognition?: BailianAsrModuleOptions
  bailianSpeechRecognitionRealtime?: BailianRealtimeModuleOptions
  bailianTranslation?: BailianQwenMtModuleConfig
}

export interface SayItCapabilityRuntime {
  readonly sourceNamespace: typeof SAYIT_AI_SDK_SOURCE_NAMESPACE
  descriptors(): readonly CapabilityDescriptor[]
  execute(
    moduleId: string,
    input: unknown,
    options?: CapabilityExecuteOptions<unknown>
  ): Promise<unknown>
  openSession(
    moduleId: string,
    input: unknown,
    options?: CapabilityExecuteOptions<unknown>
  ): Promise<CapabilityRealtimeSession<unknown, unknown>>
  cancel(requestId: string): void
  dispose(): Promise<void>
}

export interface SayItPluginCapabilityDefinition {
  moduleId: string
  kind: string
  providerIds: readonly string[]
  modelId: string
  operations: readonly string[]
  features: readonly string[]
  tags: readonly string[]
  executionModes: readonly string[]
}

export interface SayItPluginDefinitionSet {
  pluginId: string
  sourceNamespace: string
  capabilities: readonly SayItPluginCapabilityDefinition[]
}

export interface SayItPluginProviderAdapter {
  invoke?(request: { operation: string; payload: unknown }): unknown | Promise<unknown>
  realtimeStart?(input: unknown): unknown | Promise<unknown>
  realtimeAudio?(input: Uint8Array): unknown | Promise<unknown>
  realtimeFinish?(): unknown | Promise<unknown>
  realtimeStop?(): unknown | Promise<unknown>
}

export interface SayItPluginCapabilityRuntime {
  descriptors(): readonly CapabilityDescriptor[]
  execute(
    moduleId: string,
    input: unknown,
    options?: CapabilityExecuteOptions<unknown>
  ): Promise<unknown>
  openSession(
    moduleId: string,
    input: unknown,
    options?: CapabilityExecuteOptions<unknown>
  ): Promise<CapabilityRealtimeSession<unknown, unknown>>
  unregisterSource(namespace: string): Promise<number>
  handleProviderEvent(event: unknown): void
  dispose(): Promise<void>
}

interface PluginEventRouter {
  set(handler: ((event: unknown) => void | Promise<void>) | undefined): void
  handle(event: unknown): void
}

export function createSayItCapabilityRuntime(
  runtime: RuntimeContext,
  options: SayItCapabilityRuntimeOptions
): SayItCapabilityRuntime {
  const sources = normalizeSources(options.sources)
  const client = createCapabilityClient({ runtime })
  registerSources(client, sources, options)
  assertBuiltinSourceNamespace(client.list())
  let disposed = false

  const ensureActive = (): void => {
    if (disposed) throw new Error('Say-It SDK capability runtime 已销毁')
  }

  return {
    sourceNamespace: SAYIT_AI_SDK_SOURCE_NAMESPACE,
    descriptors: () => {
      ensureActive()
      return client.list()
    },
    execute: async (moduleId, input, execution) => {
      ensureActive()
      return await client.execute(moduleId, input, execution)
    },
    openSession: async (moduleId, input, execution) => {
      ensureActive()
      return await client.openSession(moduleId, input, execution)
    },
    cancel: requestId => {
      ensureActive()
      client.cancel(requestId)
    },
    dispose: async () => {
      if (disposed) return
      disposed = true
      await client.dispose()
    },
  }
}

export function validateSayItPluginCapabilityDefinitions(
  definitionsJson: string,
  sourceNamespace: string
): string {
  const definitions = parseDefinitions(definitionsJson)
  const client = createCapabilityClient({ runtime: validationRuntime() })
  registerPluginDefinitions(client, sourceNamespace, definitions)
  const descriptors = client.list()
  void client.dispose()
  return JSON.stringify(descriptors)
}

export function validateSayItPluginCapabilityRegistry(registryJson: string): string {
  const registry = parseRegistry(registryJson)
  const client = createCapabilityClient({ runtime: validationRuntime() })
  registerSources(client, new Set(SAYIT_CAPABILITY_MODULE_SOURCES), { sources: [] })
  for (const plugin of registry) {
    if (plugin.sourceNamespace !== plugin.pluginId) {
      throw new Error(
        `插件 ${plugin.pluginId} 的 source namespace 必须由宿主固定为自身 ID`
      )
    }
    registerPluginDefinitions(client, plugin.sourceNamespace, plugin.capabilities)
  }
  const descriptors = client.list().filter(descriptor => descriptor.source.kind === 'plugin')
  void client.dispose()
  return JSON.stringify(descriptors)
}

export function createSayItPluginCapabilityRuntime(
  runtime: RuntimeContext,
  sourceNamespace: string,
  definitions: readonly SayItPluginCapabilityDefinition[],
  provider: SayItPluginProviderAdapter
): SayItPluginCapabilityRuntime {
  const client = createCapabilityClient({ runtime })
  const eventRouter = createPluginEventRouter()
  registerPluginDefinitions(client, sourceNamespace, definitions, provider, eventRouter)
  let disposed = false
  const ensureActive = (): void => {
    if (disposed) throw new Error(`插件 capability runtime ${sourceNamespace} 已销毁`)
  }
  return {
    descriptors: () => {
      ensureActive()
      return client.list()
    },
    execute: async (moduleId, input, options) => {
      ensureActive()
      return await client.execute(moduleId, input, options)
    },
    openSession: async (moduleId, input, options) => {
      ensureActive()
      return await client.openSession(moduleId, input, options)
    },
    unregisterSource: async namespace => {
      ensureActive()
      return await client.unregisterSource(namespace)
    },
    handleProviderEvent: event => eventRouter.handle(event),
    dispose: async () => {
      if (disposed) return
      await client.unregisterSource(sourceNamespace)
      disposed = true
      await client.dispose()
    },
  }
}

function registerPluginDefinitions(
  client: CapabilityClient,
  sourceNamespace: string,
  definitions: readonly SayItPluginCapabilityDefinition[],
  provider: SayItPluginProviderAdapter = validationProvider(),
  eventRouter: PluginEventRouter = createPluginEventRouter()
): void {
  for (const definition of definitions) {
    const descriptor = pluginDescriptor(sourceNamespace, definition)
    const realtime = definition.executionModes.includes('realtime')
    if (realtime) {
      if (definition.kind !== 'speech-recognition') {
        throw new Error(`插件 module ${definition.moduleId} 只有语音识别可声明 realtime`)
      }
      try {
        client.registerRealtime(pluginRealtimeModule(descriptor, provider, eventRouter))
      } catch (error) {
        throw pluginRegistrationError(definition.moduleId, error)
      }
      continue
    }
    if (!definition.executionModes.some(mode => mode === 'request-response' || mode === 'event-stream')) {
      throw new Error(`插件 module ${definition.moduleId} 缺少可执行 executionMode`)
    }
    try {
      client.register(pluginExecuteModule(descriptor, provider, eventRouter))
    } catch (error) {
      throw pluginRegistrationError(definition.moduleId, error)
    }
  }
}

function pluginRegistrationError(moduleId: string, error: unknown): Error {
  const message = error instanceof Error ? error.message : String(error)
  return new Error(`插件 module ${moduleId} 注册失败：${message}`)
}

function pluginDescriptor(
  sourceNamespace: string,
  definition: SayItPluginCapabilityDefinition
): CapabilityDescriptor {
  const contract = definition.kind === 'speech-recognition'
    ? {
        input: [{ kind: 'audio' as const, required: true }],
        output: [{ kind: 'text' as const, required: true }, { kind: 'structured-data' as const }],
      }
    : definition.kind === 'translation'
      ? {
          input: [{ kind: 'text' as const, required: true, multiple: true }],
          output: [
            { kind: 'text' as const, required: true, multiple: true },
            { kind: 'structured-data' as const, required: true },
          ],
        }
      : (() => { throw new Error(`插件 capability kind 尚未开放：${definition.kind}`) })()
  const executionModes = definition.executionModes.map(mode => {
    if (!['request-response', 'event-stream', 'realtime'].includes(mode)) {
      throw new Error(`插件 module ${definition.moduleId} 使用未知 executionMode：${mode}`)
    }
    return mode as CapabilityExecutionMode
  })
  return {
    id: definition.moduleId,
    kind: definition.kind,
    source: { kind: 'plugin', namespace: sourceNamespace },
    contract,
    providerIds: definition.providerIds,
    modelId: definition.modelId,
    operations: definition.operations,
    features: definition.features,
    tags: definition.tags,
    executionModes,
  }
}

function pluginExecuteModule(
  descriptor: CapabilityDescriptor,
  provider: SayItPluginProviderAdapter,
  eventRouter: PluginEventRouter
): CapabilityModule<unknown, unknown, unknown> {
  return {
    descriptor,
    execute: async (input, context) => {
      if (typeof provider.invoke !== 'function') {
        throw new Error(`插件未实现 ${descriptor.kind} invoke adapter`)
      }
      eventRouter.set(async event => await context.emit(event))
      try {
        const operation = descriptor.kind === 'translation' ? 'translate' : 'transcribeFile'
        return await provider.invoke({ operation, payload: legacyPayload(input) })
      } finally {
        eventRouter.set(undefined)
      }
    },
  }
}

function pluginRealtimeModule(
  descriptor: CapabilityDescriptor,
  provider: SayItPluginProviderAdapter,
  eventRouter: PluginEventRouter
): CapabilityRealtimeModule<unknown, unknown, unknown, unknown> {
  return {
    descriptor,
    open: async (input, context) => {
      if (
        typeof provider.realtimeStart !== 'function'
        || typeof provider.realtimeAudio !== 'function'
        || typeof provider.realtimeFinish !== 'function'
        || typeof provider.realtimeStop !== 'function'
      ) {
        throw new Error(`插件未完整实现 realtime adapter：${descriptor.id}`)
      }
      await provider.realtimeStart(legacyPayload(input))
      let transcript = ''
      let resolveFinished: (value: unknown) => void = () => undefined
      let rejectFinished: (error: unknown) => void = () => undefined
      const finished = new Promise<unknown>((resolve, reject) => {
        resolveFinished = resolve
        rejectFinished = reject
      })
      eventRouter.set(async event => {
        await context.emit(event)
        if (!isRecord(event)) return
        if (event.type === 'final' && typeof event.text === 'string') transcript += event.text
        if (event.type === 'finished') resolveFinished({ text: transcript })
        if (event.type === 'error') {
          rejectFinished(new Error(typeof event.message === 'string' ? event.message : '插件实时能力失败'))
        }
      })
      return {
        send: async value => {
          const bytes = value instanceof Uint8Array
            ? value
            : isRecord(value) && value.bytes instanceof Uint8Array
              ? value.bytes
              : undefined
          if (!bytes) throw new Error(`插件 realtime 输入必须为 Uint8Array：${descriptor.id}`)
          await provider.realtimeAudio!(bytes)
        },
        finish: async () => {
          const immediate = await provider.realtimeFinish!()
          if (immediate !== undefined && immediate !== null) resolveFinished(immediate)
          return await finished
        },
        close: async () => {
          eventRouter.set(undefined)
          await provider.realtimeStop!()
        },
      }
    },
  }
}

function legacyPayload(input: unknown): unknown {
  if (isRecord(input) && isRecord(input.options) && 'legacyPayload' in input.options) {
    return input.options.legacyPayload
  }
  return input
}

function parseDefinitions(json: string): readonly SayItPluginCapabilityDefinition[] {
  const value: unknown = JSON.parse(json)
  if (!Array.isArray(value)) throw new Error('插件 capabilities 必须是数组')
  return value as SayItPluginCapabilityDefinition[]
}

function parseRegistry(json: string): readonly SayItPluginDefinitionSet[] {
  const value: unknown = JSON.parse(json)
  if (!Array.isArray(value)) throw new Error('插件 capability registry 必须是数组')
  return value as SayItPluginDefinitionSet[]
}

function validationProvider(): SayItPluginProviderAdapter {
  return {
    invoke: async () => ({}),
    realtimeStart: async () => undefined,
    realtimeAudio: async () => undefined,
    realtimeFinish: async () => ({}),
    realtimeStop: async () => undefined,
  }
}

function createPluginEventRouter(): PluginEventRouter {
  let handler: ((event: unknown) => void | Promise<void>) | undefined
  return {
    set: next => { handler = next },
    handle: event => { void handler?.(event) },
  }
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

function registerSources(
  client: CapabilityClient,
  sources: ReadonlySet<SayItCapabilityModuleSource>,
  options: SayItCapabilityRuntimeOptions
): void {
  if (sources.has('bailian-speech-recognition')) {
    for (const preset of bailianNonRealtimeAsrPresets) {
      client.register(createBailianAsrModule(preset, options.bailianSpeechRecognition))
    }
  }
  if (sources.has('bailian-speech-recognition-realtime')) {
    for (const preset of bailianRealtimeAsrPresets) {
      client.registerRealtime(
        createBailianRealtimeAsrModule(preset, options.bailianSpeechRecognitionRealtime)
      )
    }
  }
  if (sources.has('bailian-translation')) {
    for (const preset of Object.values(BAILIAN_QWEN_MT_PRESETS)) {
      client.register(
        createBailianQwenMtTranslationModule(preset.modelId, options.bailianTranslation)
      )
    }
  }
}

function normalizeSources(
  input: readonly SayItCapabilityModuleSource[]
): ReadonlySet<SayItCapabilityModuleSource> {
  const allowed = new Set<string>(SAYIT_CAPABILITY_MODULE_SOURCES)
  const result = new Set<SayItCapabilityModuleSource>()
  for (const source of input) {
    if (!allowed.has(source)) throw new Error(`未知 SDK capability module source：${source}`)
    result.add(source)
  }
  return result
}

function assertBuiltinSourceNamespace(descriptors: readonly CapabilityDescriptor[]): void {
  for (const descriptor of descriptors) {
    if (
      descriptor.source.kind !== 'builtin'
      || descriptor.source.namespace !== SAYIT_AI_SDK_SOURCE_NAMESPACE
    ) {
      throw new Error(`SDK module ${descriptor.id} 的 source namespace 不符合 0.2.2 契约`)
    }
  }
}
