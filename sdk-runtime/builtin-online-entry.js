const capabilitySources = Object.freeze({
  asr: 'bailian-speech-recognition',
  realtimeAsr: 'bailian-speech-recognition-realtime',
  translation: 'bailian-translation',
  volcengineAsr: 'volcengine-speech-recognition',
  volcengineRealtimeAsr: 'volcengine-speech-recognition-realtime',
  siliconflowAsr: 'siliconflow-speech-recognition',
  groqAsr: 'groq-speech-recognition',
})
const allowedCapabilitySources = new Set(Object.values(capabilitySources))

const resolveCapabilitySource = source => {
  const resolved = capabilitySources[source] ?? source
  if (!allowedCapabilitySources.has(resolved)) {
    throw new Error(`未知内置 SDK capability source：${source}`)
  }
  return resolved
}

export default host => {
  let realtimeRuntime
  let realtimeSession

  const disposeRealtime = async () => {
    if (realtimeSession) {
      await realtimeSession.close()
      realtimeSession = undefined
    }
    if (realtimeRuntime) {
      await realtimeRuntime.dispose()
      realtimeRuntime = undefined
    }
  }

  return {
    async invoke(request) {
      const operation = request.operation
      if (operation === 'capability.execute') {
        const sdk = globalThis.__sayitCreateSdkRuntime({
          sources: [resolveCapabilitySource(request.source)],
        })
        try {
          return await sdk.capabilities.execute(request.moduleId, request.input, {
            requestId: request.requestId,
            timeoutMs: request.timeoutMs,
            onEvent: event => host.emit({ type: 'sdk.capability', event }),
          })
        } finally {
          await sdk.dispose()
          globalThis.__sayitDisposeRuntimeContext()
        }
      }
      if (operation === 'groq.run') {
        const sdk = globalThis.__sayitCreateSdkRuntime({ sources: ['groq-llm'] })
        try {
          const options = {
            timeoutMs: request.timeoutMs,
          }
          if (request.emitEvents) {
            options.onEvent = event => host.emit({ type: 'sdk.groq', event })
          }
          return await sdk.groq.run(request.input, request.requestId, options)
        } finally {
          await sdk.dispose()
          globalThis.__sayitDisposeRuntimeContext()
        }
      }
      if (operation === 'groq.discover') {
        const sdk = globalThis.__sayitCreateSdkRuntime({ sources: ['groq-llm'] })
        try {
          return await sdk.groq.discover({ timeoutMs: request.timeoutMs })
        } finally {
          await sdk.dispose()
          globalThis.__sayitDisposeRuntimeContext()
        }
      }
      throw new Error(`未知内置 SDK 操作：${operation}`)
    },

    async realtimeStart(request) {
      await disposeRealtime()
      realtimeRuntime = globalThis.__sayitCreateSdkRuntime({
        sources: [resolveCapabilitySource(request.source ?? 'realtimeAsr')],
      })
      // 注意：SDK 里 openSession 的 timeoutMs 是"从 open() 起整段会话的强制超时"，
      // 内部直接 setTimeout(abort, timeoutMs)，不会随后续 send/finish 活动续期。
      // 调用方仍会传 request.timeoutMs（那是单次请求预算，默认 45 秒），这里
      // 不转发——否则超过该时长的实时听写/实时字幕会被硬切断。等待 open() 握手
      // 完成的时长由 Rust 侧 realtime_start 传给 runtime.call 的超时参数负责；
      // 会话生命周期交给 finish()/close() 自然驱动结束。
      // 插件路径的同一决策见 plugin_runtime.rs 的 __sayitPluginCapabilityOpen。
      realtimeSession = await realtimeRuntime.capabilities.openSession(
        request.moduleId,
        request.input,
        {
          requestId: request.requestId,
          onEvent: event => host.emit({ type: 'sdk.capability', event }),
        },
      )
      return null
    },

    async realtimeAudio(audio) {
      if (!realtimeSession) throw new Error('内置 SDK 实时识别尚未开始')
      await realtimeSession.send({ bytes: audio })
    },

    async realtimeFinish() {
      if (!realtimeSession) throw new Error('内置 SDK 实时识别尚未开始')
      try {
        return await realtimeSession.finish()
      } finally {
        await disposeRealtime()
        globalThis.__sayitDisposeRuntimeContext()
      }
    },

    async realtimeStop() {
      await disposeRealtime()
      globalThis.__sayitDisposeRuntimeContext()
      return null
    },
  }
}
