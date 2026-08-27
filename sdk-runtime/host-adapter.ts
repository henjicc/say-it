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

  const call = <T>(operation: string, payload: unknown = {}): T => {
    const result = JSON.parse(rawCall(operation, JSON.stringify(payload))) as HostResult
    if (!result.ok) throw new Error(result.error || `宿主调用失败：${operation}`)
    return result.value as T
  }
  const bytes = (value: ArrayBuffer | ArrayBufferView): number[] => {
    const view = value instanceof ArrayBuffer
      ? new Uint8Array(value)
      : new Uint8Array(value.buffer, value.byteOffset, value.byteLength)
    return Array.from(view)
  }
  const headerRecord = (headers?: HeadersInit): Record<string, string> => {
    const result: Record<string, string> = {}
    new Headers(headers).forEach((value, name) => { result[name] = value })
    return result
  }
  const requestBody = (body: BodyInit | null | undefined): { payload: Record<string, unknown>; contentType?: string } => {
    if (body === undefined || body === null) return { payload: {} }
    if (typeof body === 'string') return { payload: { bodyText: body } }
    if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) return { payload: { bodyBytes: bytes(body) } }
    const serialized = (body as unknown as {
      __sayitSerializeBody?: () => { bytes: Uint8Array; contentType: string }
    }).__sayitSerializeBody?.()
    if (serialized) return { payload: { bodyBytes: Array.from(serialized.bytes) }, contentType: serialized.contentType }
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

  const activeStreams = new Set<string>()
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
        activeStreams.add(opened.streamId)
        let closed = false
        const close = (): void => {
          if (closed) return
          closed = true
          activeStreams.delete(opened.streamId)
          call('http.stream.close', { streamId: opened.streamId })
        }
        const onAbort = (): void => close()
        init.signal?.addEventListener('abort', onAbort, { once: true })
        const responseBody = new HostReadableStream(() => ({
          read: async () => {
            if (init.signal?.aborted) {
              close()
              throw abortError()
            }
            let result: StreamReadResult
            try {
              result = call<StreamReadResult>('http.stream.read', { streamId: opened.streamId })
            } catch (error) {
              close()
              throw error
            }
            if (result.done) {
              close()
              init.signal?.removeEventListener('abort', onAbort)
              return { done: true, value: undefined }
            }
            return { done: false, value: new Uint8Array(result.bytes ?? []) }
          },
          cancel: async () => close(),
          releaseLock: () => undefined,
        }))
        return new Response(responseBody, { status: opened.status, headers: opened.headers, url: opened.url } as ResponseInit)
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
      read: async (ref: string) => {
        const result = call<{ bytes: number[]; mimeType: string; filename: string }>('media.read', { ref })
        return { ...result, bytes: new Uint8Array(result.bytes) }
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
    for (const streamId of Array.from(activeStreams)) call('http.stream.close', { streamId })
    for (const connectionId of Array.from(websocketQueues.keys())) call('websocket.close', { connectionId })
    for (const timerId of Array.from(timerCallbacks.keys())) call('timer.close', { timerId })
    activeStreams.clear()
    websocketQueues.clear()
    timerCallbacks.clear()
  }
})()
