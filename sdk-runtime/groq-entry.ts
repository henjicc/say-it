import {
  discoverGroqModels,
  GROQ_DEFAULT_MODEL_ID,
  GROQ_LLM_MODULE_DESCRIPTOR,
  GROQ_PROVIDER_ID,
  runGroqChatStream,
  type DiscoveredModelItem,
  type GroqChatRequest,
  type LlmChatStreamOutcome,
  type LlmStreamEmitter,
  type RuntimeContext,
} from '@henjicc/ai-sdk/llm/groq'

export const SAYIT_GROQ_MODULE_SOURCE = 'groq-llm' as const

export function sayItGroqModuleDescriptorJson(): string {
  return JSON.stringify([GROQ_LLM_MODULE_DESCRIPTOR])
}

export interface SayItGroqRunOptions {
  timeoutMs?: number
  onEvent?: LlmStreamEmitter
}

export interface SayItGroqRuntime {
  readonly providerId: typeof GROQ_PROVIDER_ID
  readonly defaultModelId: typeof GROQ_DEFAULT_MODEL_ID
  run(request: GroqChatRequest, taskId: string, options?: SayItGroqRunOptions): Promise<LlmChatStreamOutcome>
  discover(options?: { signal?: AbortSignal; timeoutMs?: number }): Promise<DiscoveredModelItem[]>
  cancel(taskId: string): void
  dispose(): Promise<void>
}

export function createSayItGroqRuntime(runtime: RuntimeContext): SayItGroqRuntime {
  const active = new Map<string, AbortController>()
  const discoveries = new Set<AbortController>()
  let disposed = false

  const ensureActive = (): void => {
    if (disposed) throw new Error('Say-It SDK Groq runtime 已销毁')
  }

  return {
    providerId: GROQ_PROVIDER_ID,
    defaultModelId: GROQ_DEFAULT_MODEL_ID,
    run: async (request, taskId, options = {}) => {
      ensureActive()
      if (active.has(taskId)) throw new Error(`Groq task 已在执行：${taskId}`)
      const controller = new AbortController()
      active.set(taskId, controller)
      try {
        return await runGroqChatStream(
          request,
          taskId,
          options.onEvent ?? (() => undefined),
          runtime,
          { signal: controller.signal, timeoutMs: options.timeoutMs }
        )
      } finally {
        active.delete(taskId)
      }
    },
    discover: async (options = {}) => {
      ensureActive()
      const controller = new AbortController()
      const onAbort = (): void => controller.abort(options.signal?.reason)
      if (options.signal?.aborted) onAbort()
      else options.signal?.addEventListener('abort', onAbort, { once: true })
      discoveries.add(controller)
      try {
        return await discoverGroqModels(runtime, {
          ...options,
          signal: controller.signal,
        })
      } finally {
        discoveries.delete(controller)
        options.signal?.removeEventListener('abort', onAbort)
      }
    },
    cancel: taskId => active.get(taskId)?.abort(),
    dispose: async () => {
      if (disposed) return
      disposed = true
      for (const controller of active.values()) controller.abort()
      for (const controller of discoveries) controller.abort()
      active.clear()
      discoveries.clear()
    },
  }
}
