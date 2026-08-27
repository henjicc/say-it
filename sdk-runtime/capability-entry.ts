import {
  createCapabilityClient,
  type CapabilityClient,
  type CapabilityDescriptor,
  type CapabilityExecuteOptions,
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
      throw new Error(`SDK module ${descriptor.id} 的 source namespace 不符合 0.2.1 契约`)
    }
  }
}
