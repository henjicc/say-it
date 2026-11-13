import {
  discoverModels,
  runLlmChatStream,
  type DiscoveredModelItem,
  type LlmChatRequestDto,
  type LlmChatStreamOutcome,
  type LlmStreamEmitter,
  type RuntimeContext,
} from '@henjicc/ai-sdk/llm/streaming'
import { GROQ_LLM_MODULE_DESCRIPTOR } from '@henjicc/ai-sdk/llm/groq'
import { findLlmModelCatalogEntry, findLlmProviderPreset } from '@henjicc/ai-sdk/llm'

export const SAYIT_LLM_MODULE_SOURCE = 'llm' as const

/** SDK 内置的 LLM module 坐标；插件声明同一坐标时由插件校验拒绝。 */
export function sayItBuiltinLlmModuleDescriptorJson(): string {
  return JSON.stringify([GROQ_LLM_MODULE_DESCRIPTOR])
}

export interface SayItLlmRunOptions {
  timeoutMs?: number
  onEvent?: LlmStreamEmitter
  /** SDK 目录里没有这个供应商与模型时使用的能力（手动添加的模型、自定义地址等）。 */
  fallbackCapabilities?: LlmChatRequestDto['capabilities']
}

export interface SayItLlmDiscoverRequest {
  providerId: string
  baseUrl?: string
  timeoutMs?: number
}

/** 内置大模型的唯一执行入口：协议、地址、思考与联网参数都由 SDK 按供应商与模型决定。 */
export interface SayItLlmRuntime {
  run(request: LlmChatRequestDto, taskId: string, options?: SayItLlmRunOptions): Promise<LlmChatStreamOutcome>
  discover(request: SayItLlmDiscoverRequest): Promise<DiscoveredModelItem[]>
  dispose(): Promise<void>
}

/** 与 SDK 补全能力的条件一致：预设和目录都有这个模型时由 SDK 补全，否则用调用方的能力。 */
function knownToSdk(request: LlmChatRequestDto): boolean {
  const preset = findLlmProviderPreset(request.providerId)
  return preset !== null && findLlmModelCatalogEntry(request.modelId, preset.providerId) !== null
}

export function createSayItLlmRuntime(runtime: RuntimeContext): SayItLlmRuntime {
  const active = new Set<AbortController>()
  let disposed = false

  const track = async <T>(work: (signal: AbortSignal) => Promise<T>): Promise<T> => {
    if (disposed) throw new Error('Say-It SDK LLM runtime 已销毁')
    const controller = new AbortController()
    active.add(controller)
    try {
      return await work(controller.signal)
    } finally {
      active.delete(controller)
    }
  }

  return {
    run: (request, taskId, options = {}) => track(signal => runLlmChatStream(
      knownToSdk(request) || !options.fallbackCapabilities
        ? request
        : { ...request, capabilities: options.fallbackCapabilities },
      taskId,
      options.onEvent ?? (() => undefined),
      runtime,
      {},
      { signal, timeoutMs: options.timeoutMs },
    )),
    discover: request => track(signal => discoverModels(request.providerId, request.baseUrl, runtime, {
      signal,
      timeoutMs: request.timeoutMs,
    })),
    dispose: async () => {
      if (disposed) return
      disposed = true
      for (const controller of active) controller.abort()
      active.clear()
    },
  }
}
