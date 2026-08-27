const capabilitySources = Object.freeze({
  asr: 'bailian-speech-recognition',
  realtimeAsr: 'bailian-speech-recognition-realtime',
  translation: 'bailian-translation',
})

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
          sources: [capabilitySources[request.source]],
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
        sources: [capabilitySources.realtimeAsr],
      })
      realtimeSession = await realtimeRuntime.capabilities.openSession(
        request.moduleId,
        request.input,
        {
          requestId: request.requestId,
          timeoutMs: request.timeoutMs,
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
