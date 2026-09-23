(() => {
  type HostResult = { ok: true; value: unknown } | { ok: false; error: string }
  type HostEvent = {
    type?: string
    connectionId?: string
    timerId?: string
    text?: string
    bytes?: number[]
    message?: string
  }

  const rawCall = (globalThis as typeof globalThis & {
    __sayitHostCall?: (operation: string, payload: string) => string
  }).__sayitHostCall
  if (typeof rawCall !== 'function') throw new Error('SDK RuntimeContext 缺少 Rust Host API')
  const rawStoreRequestBody = (globalThis as typeof globalThis & {
    __sayitHostStoreRequestBody?: (body: Uint8Array) => string
  }).__sayitHostStoreRequestBody
  if (typeof rawStoreRequestBody !== 'function') throw new Error('SDK RuntimeContext 缺少二进制请求体 Host API')

  const call = <T>(operation: string, payload: unknown = {}): T => {
    const result = JSON.parse(rawCall(operation, JSON.stringify(payload))) as HostResult
    if (!result.ok) throw new Error(result.error || `宿主调用失败：${operation}`)
    return result.value as T
  }
  const byteView = (value: ArrayBuffer | ArrayBufferView): Uint8Array =>
    value instanceof ArrayBuffer
      ? new Uint8Array(value)
      : new Uint8Array(value.buffer, value.byteOffset, value.byteLength)
  const storeRequestBody = (value: ArrayBuffer | ArrayBufferView): Record<string, unknown> => ({
    bodyBufferId: rawStoreRequestBody(byteView(value)),
  })
  const headerRecord = (headers?: HeadersInit): Record<string, string> => {
    const result: Record<string, string> = {}
    new Headers(headers).forEach((value, name) => { result[name] = value })
    return result
  }
  const requestBody = (body: BodyInit | null | undefined): { payload: Record<string, unknown>; contentType?: string } => {
    if (body === undefined || body === null) return { payload: {} }
    if (typeof body === 'string') return { payload: { bodyText: body } }
    if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) return { payload: storeRequestBody(body) }
    const serialized = (body as unknown as {
      __sayitSerializeBody?: () => { bytes: Uint8Array; contentType: string }
    }).__sayitSerializeBody?.()
    if (serialized) return { payload: storeRequestBody(serialized.bytes), contentType: serialized.contentType }
    throw new TypeError('QuickJS Transport 只接受 string/ArrayBuffer/TypedArray 请求体')
  }
  const abortError = (): Error => Object.assign(new Error('操作已取消'), { name: 'AbortError' })

  interface StreamOpenResult {
    streamId: string
    status: number
    headers: Record<string, string>
    url: string
  }
  interface StreamReadResult { done: boolean; bytes?: number[] }
  interface MediaDescription { size: number; mimeType: string; filename: string }
  interface MediaChunk { bytes: number[] }
  const MEDIA_CHUNK_BYTES = 64 * 1024

  const activeStreams = new Set<string>()
  const activeUploads = new Set<string>()
  const timerCallbacks = new Map<string, { handler: (...args: unknown[]) => void; args: unknown[] }>()
  const websocketQueues = new Map<string, {
    values: Array<{ data: string | Uint8Array }>
    waiters: Array<{
      resolve: (value: IteratorResult<{ data: string | Uint8Array }>) => void
      reject: (error: Error) => void
    }>
    closed: boolean
    error?: Error
  }>()

  const dispatchHostEvent = (event: HostEvent): void => {
    if (event.type === 'timerFired' && event.timerId) {
      const timer = timerCallbacks.get(event.timerId)
      timerCallbacks.delete(event.timerId)
      timer?.handler(...timer.args)
      return
    }
    const id = event.connectionId
    if (!id) return
    const queue = websocketQueues.get(id)
    if (!queue) return
    if (event.type === 'websocketMessage') {
      const value = { data: event.text ?? new Uint8Array(event.bytes ?? []) }
      const waiter = queue.waiters.shift()
      if (waiter) waiter.resolve({ done: false, value })
      else queue.values.push(value)
    } else if (event.type === 'websocketError') {
      queue.error = new Error(event.message ?? 'WebSocket 宿主错误')
      queue.closed = true
      for (const waiter of queue.waiters.splice(0)) waiter.reject(queue.error)
    } else if (event.type === 'websocketClose') {
      queue.closed = true
      for (const waiter of queue.waiters.splice(0)) waiter.resolve({ done: true, value: undefined })
    }
  }

  const target = globalThis as typeof globalThis & {
    __SayItReadableStream?: new (reader: () => {
      read(): Promise<ReadableStreamReadResult<Uint8Array>>
      cancel(reason?: unknown): Promise<void>
      releaseLock(): void
    }) => ReadableStream<Uint8Array>
    __sayitDispatchHostEvent?: (event: HostEvent) => void
    __sayitDispatchHostEventJson?: (event: string) => void
    __sayitCreateRuntimeContext?: () => unknown
    __sayitDisposeRuntimeContext?: () => void
    setTimeout?: (handler: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) => number
    clearTimeout?: (handle?: number) => void
  }
  const HostReadableStream = target.__SayItReadableStream
  if (!HostReadableStream) throw new Error('SDK RuntimeContext 缺少 ReadableStream 兼容层')
  target.__sayitDispatchHostEvent = dispatchHostEvent
  target.__sayitDispatchHostEventJson = event => dispatchHostEvent(JSON.parse(event) as HostEvent)
  if (typeof target.setTimeout !== 'function') {
    target.setTimeout = (handler, delay = 0, ...args) => {
      if (typeof handler !== 'function') throw new TypeError('QuickJS setTimeout 只接受函数')
      const { timerId } = call<{ timerId: string }>('timer.open', { millis: delay })
      timerCallbacks.set(timerId, { handler: handler as (...args: unknown[]) => void, args })
      return timerId as unknown as number
    }
  }
  if (typeof target.clearTimeout !== 'function') {
    target.clearTimeout = handle => {
      if (handle === undefined) return
      const timerId = String(handle)
      timerCallbacks.delete(timerId)
      call('timer.close', { timerId })
    }
  }

  const responseFromHost = (opened: StreamOpenResult, signal?: AbortSignal | null): Response => {
    activeStreams.add(opened.streamId)
    let closed = false
    const close = (): void => {
      if (closed) return
      closed = true
      signal?.removeEventListener('abort', close)
      activeStreams.delete(opened.streamId)
      call('http.stream.close', { streamId: opened.streamId })
    }
    signal?.addEventListener('abort', close, { once: true })
    if (signal?.aborted) { close(); throw abortError() }
    const body = new HostReadableStream(() => ({
      read: async () => {
        if (signal?.aborted) { close(); throw abortError() }
        try {
          const result = call<StreamReadResult>('http.stream.read', { streamId: opened.streamId })
          if (result.done) { close(); return { done: true, value: undefined } }
          return { done: false, value: new Uint8Array(result.bytes ?? []) }
        } catch (error) { close(); throw error }
      },
      cancel: async () => close(),
      releaseLock: () => undefined,
    }))
    return new Response(body, { status: opened.status, headers: opened.headers, url: opened.url } as ResponseInit)
  }

  // 让出 QuickJS 作业循环，使 SDK AbortSignal/计时器在网络背压期间也能执行。
  const uploadTick = async (signal?: AbortSignal | null): Promise<void> => {
    if (signal?.aborted) throw abortError()
    await new Promise<void>(resolve => setTimeout(resolve, 5))
    if (signal?.aborted) throw abortError()
  }
  const finishUpload = async (uploadId: string, signal?: AbortSignal | null): Promise<Response> => {
    while (true) {
      if (signal?.aborted) throw abortError()
      const result = call<{ ready: false } | { ready: true; response: StreamOpenResult }>('http.upload.finish', { uploadId })
      if (result.ready) return responseFromHost(result.response, signal)
      await uploadTick(signal)
    }
  }

  target.__sayitCreateRuntimeContext = () => ({
    transport: {
      fetch: async (url: string, init: RequestInit = {}): Promise<Response> => {
        if (init.signal?.aborted) throw abortError()
        const headers = headerRecord(init.headers)
        const requestBodyData = requestBody(init.body)
        if (requestBodyData.contentType && !headers['content-type']) headers['content-type'] = requestBodyData.contentType
        const opened = call<StreamOpenResult>('http.stream.open', {
          url,
          method: init.method ?? 'GET',
          headers,
          ...requestBodyData.payload,
        })
        return responseFromHost(opened, init.signal)
      },
      fetchStream: async (url: string, init: Omit<RequestInit, 'body'> & {
        body: AsyncIterable<Uint8Array>; contentLength: number
      }): Promise<Response> => {
        const iterator = init.body[Symbol.asyncIterator]()
        let uploadId: string | undefined
        const close = (): void => {
          if (!uploadId) return
          activeUploads.delete(uploadId)
          call('http.upload.close', { uploadId })
        }
        try {
          if (init.signal?.aborted) throw abortError()
          if (!Number.isSafeInteger(init.contentLength) || init.contentLength < 0) throw new TypeError('非法流式请求体长度')
          uploadId = call<{ uploadId: string }>('http.upload.open', {
            url, method: init.method ?? 'POST', headers: headerRecord(init.headers), contentLength: init.contentLength,
          }).uploadId
          activeUploads.add(uploadId)
          init.signal?.addEventListener('abort', close, { once: true })
          while (true) {
            if (init.signal?.aborted) throw abortError()
            const next = await iterator.next()
            if (init.signal?.aborted) throw abortError()
            if (next.done) break
            if (!(next.value instanceof Uint8Array)) throw new TypeError('上传分块必须是 Uint8Array')
            for (let offset = 0; offset < next.value.length; offset += MEDIA_CHUNK_BYTES) {
              if (init.signal?.aborted) throw abortError()
              while (true) {
                const progress = call<{ ready: boolean; response?: StreamOpenResult }>('http.upload.write', {
                  uploadId, ...storeRequestBody(next.value.subarray(offset, offset + MEDIA_CHUNK_BYTES)),
                })
                if (progress.response) return responseFromHost(progress.response, init.signal)
                if (progress.ready) break
                await uploadTick(init.signal)
              }
            }
          }
          return await finishUpload(uploadId, init.signal)
        } finally {
          init.signal?.removeEventListener('abort', close)
          close()
          await iterator.return?.()
        }
      },
      uploadFile: async (url: string, init: {
        ref: string; fields: readonly (readonly [string, string])[]; fileField: string; maxBytes: number; signal: AbortSignal
      }): Promise<Response> => {
        if (init.signal.aborted) throw abortError()
        const opened = call<{ uploadId: string } | { error: { code: string; message: string; details: unknown } }>('http.file.open', {
          url, ref: init.ref, fields: init.fields, fileField: init.fileField, maxBytes: init.maxBytes,
        })
        if ('error' in opened) throw Object.assign(new Error(opened.error.message), opened.error)
        activeUploads.add(opened.uploadId)
        try {
          return await finishUpload(opened.uploadId, init.signal)
        } finally {
          activeUploads.delete(opened.uploadId)
          call('http.upload.close', { uploadId: opened.uploadId })
        }
      },
    },
    realtime: {
      connect: async (url: string, options: { protocols?: string | readonly string[]; headers?: Record<string, string>; signal?: AbortSignal } = {}) => {
        if (options.signal?.aborted) throw abortError()
        const opened = call<{ connectionId: string }>('websocket.open', {
          url,
          protocols: options.protocols,
          headers: options.headers,
        })
        const queue = { values: [], waiters: [], closed: false } as NonNullable<ReturnType<typeof websocketQueues.get>>
        websocketQueues.set(opened.connectionId, queue)
        let closed = false
        const close = async (code?: number, reason?: string): Promise<void> => {
          if (closed) return
          closed = true
          queue.closed = true
          websocketQueues.delete(opened.connectionId)
          call('websocket.close', { connectionId: opened.connectionId, code, reason })
          for (const waiter of queue.waiters.splice(0)) waiter.resolve({ done: true, value: undefined })
        }
        options.signal?.addEventListener('abort', () => { void close() }, { once: true })
        return {
          messages: {
            [Symbol.asyncIterator]() {
              return {
                next: async (): Promise<IteratorResult<{ data: string | Uint8Array }>> => {
                  if (queue.values.length > 0) return { done: false, value: queue.values.shift()! }
                  if (queue.error) throw queue.error
                  if (queue.closed) return { done: true, value: undefined }
                  return await new Promise((resolve, reject) => queue.waiters.push({ resolve, reject }))
                },
              }
            },
          },
          send: async (data: string | Uint8Array) => call('websocket.send', typeof data === 'string'
            ? { connectionId: opened.connectionId, text: data }
            : { connectionId: opened.connectionId, bytes: Array.from(data) }),
          close,
        }
      },
    },
    media: {
      describe: async (ref: string) => call<MediaDescription>('media.describe', { ref }),
      readChunk: async (ref: string, offset: number, length: number) =>
        new Uint8Array(call<MediaChunk>('media.readChunk', { ref, offset, length }).bytes),
      read: async (ref: string) => {
        const description = call<MediaDescription>('media.describe', { ref })
        if (!Number.isSafeInteger(description.size) || description.size < 0) {
          throw new Error('宿主返回了非法媒体大小')
        }
        // 保留旧整文件读取的内存保护；流式入口不受此限额影响。
        if (description.size > 10 * 1024 * 1024) throw Object.assign(new Error('整文件读取不能超过 10 MiB'), {
          code: 'media_too_large', details: { actualBytes: description.size, maxBytes: 10 * 1024 * 1024 },
        })
        const bytes = new Uint8Array(description.size)
        for (let offset = 0; offset < bytes.byteLength;) {
          const length = Math.min(MEDIA_CHUNK_BYTES, bytes.byteLength - offset)
          const chunk = call<MediaChunk>('media.readChunk', {
            ref,
            offset,
            length,
          }).bytes
          if (chunk.length === 0 || chunk.length > length) {
            throw new Error('宿主媒体分块长度不符合声明')
          }
          bytes.set(chunk, offset)
          offset += chunk.length
        }
        return { ...description, bytes }
      },
    },
    credentials: {
      get: async (scope: string, providerId: string) => {
        const result = call<{ value?: string }>('credential.get', { scope, providerId })
        return result.value ?? undefined
      },
    },
    logger: {
      info: (message: string, context?: unknown) => call('runtime.log', { level: 'info', message, context }),
      warn: (message: string, context?: unknown) => call('runtime.log', { level: 'warn', message, context }),
      error: (message: string, context?: unknown) => call('runtime.log', { level: 'error', message, context }),
    },
    tracer: {
      startSpan: (name: string, attributes?: Record<string, unknown>) => {
        const { spanId } = call<{ spanId: string }>('runtime.trace.start', { name, attributes })
        let ended = false
        return { end: (error?: unknown) => {
          if (ended) return
          ended = true
          call('runtime.trace.end', { spanId, error })
        } }
      },
    },
  })

  target.__sayitDisposeRuntimeContext = () => {
    call('media.releaseAll')
    for (const uploadId of Array.from(activeUploads)) call('http.upload.close', { uploadId })
    for (const streamId of Array.from(activeStreams)) call('http.stream.close', { streamId })
    for (const connectionId of Array.from(websocketQueues.keys())) call('websocket.close', { connectionId })
    for (const timerId of Array.from(timerCallbacks.keys())) call('timer.close', { timerId })
    activeStreams.clear()
    activeUploads.clear()
    websocketQueues.clear()
    timerCallbacks.clear()
  }
})()
