(() => {
  type Listener = (event: { type: 'abort' }) => void

  class SayItAbortSignal {
    aborted = false
    reason: unknown
    onabort: Listener | null = null
    private listeners = new Set<Listener>()

    addEventListener(type: string, listener: Listener): void {
      if (type === 'abort') this.listeners.add(listener)
    }

    removeEventListener(type: string, listener: Listener): void {
      if (type === 'abort') this.listeners.delete(listener)
    }

    dispatchAbort(reason?: unknown): void {
      if (this.aborted) return
      this.aborted = true
      this.reason = reason
      const event = { type: 'abort' as const }
      this.onabort?.(event)
      for (const listener of this.listeners) listener(event)
      this.listeners.clear()
    }
  }

  class SayItAbortController {
    readonly signal = new SayItAbortSignal()
    abort(reason?: unknown): void {
      this.signal.dispatchAbort(reason)
    }
  }

  class SayItHeaders {
    private readonly values = new Map<string, string>()

    constructor(init?: HeadersInit) {
      if (!init) return
      if (Array.isArray(init)) {
        for (const [name, value] of init) this.set(name, value)
      } else if (typeof (init as Headers).forEach === 'function') {
        ;(init as Headers).forEach((value, name) => this.set(name, value))
      } else {
        for (const [name, value] of Object.entries(init)) this.set(name, value)
      }
    }

    append(name: string, value: string): void {
      const key = name.toLowerCase()
      const current = this.values.get(key)
      this.values.set(key, current ? `${current}, ${value}` : String(value))
    }

    get(name: string): string | null {
      return this.values.get(name.toLowerCase()) ?? null
    }

    has(name: string): boolean {
      return this.values.has(name.toLowerCase())
    }

    set(name: string, value: string): void {
      this.values.set(name.toLowerCase(), String(value))
    }

    delete(name: string): void {
      this.values.delete(name.toLowerCase())
    }

    forEach(callback: (value: string, name: string) => void): void {
      for (const [name, value] of this.values) callback(value, name)
    }

    entries(): IterableIterator<[string, string]> {
      return this.values.entries()
    }

    [Symbol.iterator](): IterableIterator<[string, string]> {
      return this.entries()
    }
  }

  interface SayItReader {
    read(): Promise<ReadableStreamReadResult<Uint8Array>>
    cancel(reason?: unknown): Promise<void>
    releaseLock(): void
  }

  class SayItReadableStream {
    private locked = false
    constructor(private readonly createReader: () => SayItReader) {}

    getReader(): SayItReader {
      if (this.locked) throw new TypeError('ReadableStream 已被锁定')
      this.locked = true
      const reader = this.createReader()
      let released = false
      return {
        read: () => reader.read(),
        cancel: reason => reader.cancel(reason),
        releaseLock: () => {
          if (released) return
          released = true
          this.locked = false
          reader.releaseLock()
        },
      }
    }
  }

  class SayItTextDecoder {
    private pending: number[] = []
    constructor(label = 'utf-8') {
      if (!/^utf-?8$/i.test(String(label))) throw new RangeError('仅支持 UTF-8 TextDecoder')
    }

    decode(input: ArrayBufferView | ArrayBuffer = new Uint8Array(), options: { stream?: boolean } = {}): string {
      const bytes = input instanceof ArrayBuffer
        ? new Uint8Array(input)
        : new Uint8Array(input.buffer, input.byteOffset, input.byteLength)
      const source = this.pending.concat(Array.from(bytes))
      this.pending = []
      let output = ''
      for (let index = 0; index < source.length;) {
        const first = source[index]
        let width = 1
        let codePoint = first
        if (first >= 0xc2 && first <= 0xdf) {
          width = 2
          codePoint = first & 0x1f
        } else if (first >= 0xe0 && first <= 0xef) {
          width = 3
          codePoint = first & 0x0f
        } else if (first >= 0xf0 && first <= 0xf4) {
          width = 4
          codePoint = first & 0x07
        } else if (first >= 0x80) {
          output += '\uFFFD'
          index += 1
          continue
        }
        if (index + width > source.length) {
          if (options.stream) this.pending = source.slice(index)
          else output += '\uFFFD'
          break
        }
        let valid = true
        for (let offset = 1; offset < width; offset += 1) {
          const next = source[index + offset]
          if ((next & 0xc0) !== 0x80) {
            valid = false
            break
          }
          codePoint = (codePoint << 6) | (next & 0x3f)
        }
        const minimum = width === 2 ? 0x80 : width === 3 ? 0x800 : width === 4 ? 0x10000 : 0
        if (!valid || codePoint < minimum || codePoint > 0x10ffff || (codePoint >= 0xd800 && codePoint <= 0xdfff)) {
          output += '\uFFFD'
          index += 1
          continue
        }
        output += String.fromCodePoint(codePoint)
        index += width
      }
      return output
    }
  }

  class SayItTextEncoder {
    encode(input = ''): Uint8Array {
      const bytes: number[] = []
      for (const character of String(input)) {
        const point = character.codePointAt(0) ?? 0
        if (point <= 0x7f) bytes.push(point)
        else if (point <= 0x7ff) bytes.push(0xc0 | (point >> 6), 0x80 | (point & 0x3f))
        else if (point <= 0xffff) bytes.push(0xe0 | (point >> 12), 0x80 | ((point >> 6) & 0x3f), 0x80 | (point & 0x3f))
        else bytes.push(0xf0 | (point >> 18), 0x80 | ((point >> 12) & 0x3f), 0x80 | ((point >> 6) & 0x3f), 0x80 | (point & 0x3f))
      }
      return new Uint8Array(bytes)
    }
  }

  const concatBytes = (parts: Uint8Array[]): Uint8Array => {
    const output = new Uint8Array(parts.reduce((length, part) => length + part.byteLength, 0))
    let offset = 0
    for (const part of parts) {
      output.set(part, offset)
      offset += part.byteLength
    }
    return output
  }

  class SayItBlob {
    readonly type: string
    private readonly data: Uint8Array

    constructor(parts: Array<string | ArrayBuffer | ArrayBufferView | SayItBlob> = [], options: { type?: string } = {}) {
      const encoder = new SayItTextEncoder()
      this.data = concatBytes(parts.map(part => {
        if (part instanceof SayItBlob) return part.__sayitBytes()
        if (typeof part === 'string') return encoder.encode(part)
        return part instanceof ArrayBuffer
          ? new Uint8Array(part)
          : new Uint8Array(part.buffer, part.byteOffset, part.byteLength)
      }))
      this.type = String(options.type ?? '').toLowerCase()
    }

    get size(): number { return this.data.byteLength }

    async arrayBuffer(): Promise<ArrayBuffer> {
      const copy = this.__sayitBytes()
      return copy.buffer.slice(copy.byteOffset, copy.byteOffset + copy.byteLength) as ArrayBuffer
    }

    __sayitBytes(): Uint8Array { return new Uint8Array(this.data) }
  }

  class SayItFormData {
    private readonly values: Array<{ name: string; value: string | SayItBlob; filename?: string }> = []
    private static sequence = 0

    append(name: string, value: string | SayItBlob, filename?: string): void {
      if (typeof value !== 'string' && !(value instanceof SayItBlob)) {
        throw new TypeError('QuickJS FormData 只接受字符串或 Blob')
      }
      this.values.push({ name: String(name), value, filename })
    }

    get(name: string): string | SayItBlob | null {
      return this.values.find(value => value.name === name)?.value ?? null
    }

    __sayitSerializeBody(): { bytes: Uint8Array; contentType: string } {
      const boundary = `----sayit-sdk-${Date.now()}-${SayItFormData.sequence++}`
      const encoder = new SayItTextEncoder()
      const quote = (value: string): string => value.replace(/[\r\n"]/g, character =>
        character === '"' ? '%22' : character === '\r' ? '%0D' : '%0A')
      const parts: Uint8Array[] = []
      for (const entry of this.values) {
        let disposition = `Content-Disposition: form-data; name="${quote(entry.name)}"`
        if (entry.value instanceof SayItBlob) {
          disposition += `; filename="${quote(entry.filename ?? 'blob')}"`
          parts.push(encoder.encode(`--${boundary}\r\n${disposition}\r\nContent-Type: ${entry.value.type || 'application/octet-stream'}\r\n\r\n`))
          parts.push(entry.value.__sayitBytes())
        } else {
          parts.push(encoder.encode(`--${boundary}\r\n${disposition}\r\n\r\n${entry.value}`))
        }
        parts.push(encoder.encode('\r\n'))
      }
      parts.push(encoder.encode(`--${boundary}--\r\n`))
      return { bytes: concatBytes(parts), contentType: `multipart/form-data; boundary=${boundary}` }
    }
  }

  class SayItURL {
    readonly href: string
    readonly protocol: string
    readonly hostname: string
    readonly pathname: string
    readonly origin: string

    constructor(input: string, base?: string | SayItURL) {
      const raw = String(input)
      const absolute = /^[A-Za-z][A-Za-z0-9+.-]*:\/\//.test(raw)
        ? raw
        : SayItURL.resolveRelative(raw, base)
      const matched = absolute.match(/^([A-Za-z][A-Za-z0-9+.-]*:)(?:\/\/)([^/?#]*)([^?#]*)(?:\?[^#]*)?(?:#.*)?$/)
      if (!matched || !matched[2]) throw new TypeError('无效 URL')
      this.href = absolute
      this.protocol = matched[1].toLowerCase()
      this.hostname = matched[2].replace(/^.*@/, '').replace(/:\d+$/, '')
      this.pathname = matched[3] || '/'
      this.origin = `${this.protocol}//${matched[2]}`
    }

    private static resolveRelative(input: string, base?: string | SayItURL): string {
      if (base === undefined) throw new TypeError('相对 URL 缺少 base')
      const baseUrl = base instanceof SayItURL ? base : new SayItURL(String(base))
      if (input.startsWith('/')) return `${baseUrl.origin}${input}`
      const directory = baseUrl.pathname.replace(/[^/]*$/, '')
      return `${baseUrl.origin}${directory}${input}`
    }

    toString(): string { return this.href }
    toJSON(): string { return this.href }
  }

  class SayItResponse {
    readonly status: number
    readonly statusText: string
    readonly headers: SayItHeaders
    readonly ok: boolean
    readonly body: SayItReadableStream | null
    readonly url: string
    bodyUsed = false

    constructor(body: SayItReadableStream | Uint8Array | string | null = null, init: ResponseInit & { url?: string } = {}) {
      this.status = init.status ?? 200
      this.statusText = init.statusText ?? ''
      this.headers = new SayItHeaders(init.headers)
      this.ok = this.status >= 200 && this.status < 300
      this.url = init.url ?? ''
      if (body instanceof SayItReadableStream || body === null) this.body = body
      else {
        const bytes = typeof body === 'string' ? new SayItTextEncoder().encode(body) : body
        let consumed = false
        this.body = new SayItReadableStream(() => ({
          read: async () => consumed
            ? { done: true, value: undefined }
            : (consumed = true, { done: false, value: bytes }),
          cancel: async () => { consumed = true },
          releaseLock: () => undefined,
        }))
      }
    }

    async arrayBuffer(): Promise<ArrayBuffer> {
      const bytes = await this.consume()
      return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer
    }

    async text(): Promise<string> {
      return new SayItTextDecoder().decode(await this.consume())
    }

    async json(): Promise<unknown> {
      return JSON.parse(await this.text()) as unknown
    }

    private async consume(): Promise<Uint8Array> {
      if (this.bodyUsed) throw new TypeError('Response body 已读取')
      this.bodyUsed = true
      if (!this.body) return new Uint8Array()
      const reader = this.body.getReader()
      const chunks: Uint8Array[] = []
      let length = 0
      try {
        while (true) {
          const result = await reader.read()
          if (result.done) break
          chunks.push(result.value)
          length += result.value.length
        }
      } finally {
        reader.releaseLock()
      }
      const output = new Uint8Array(length)
      let offset = 0
      for (const chunk of chunks) {
        output.set(chunk, offset)
        offset += chunk.length
      }
      return output
    }
  }

  const target = globalThis as unknown as Record<string, unknown>
  if (typeof target.AbortController !== 'function') target.AbortController = SayItAbortController
  if (typeof target.Headers !== 'function') target.Headers = SayItHeaders
  if (typeof target.ReadableStream !== 'function') target.ReadableStream = SayItReadableStream
  if (typeof target.Response !== 'function') target.Response = SayItResponse
  if (typeof target.TextDecoder !== 'function') target.TextDecoder = SayItTextDecoder
  if (typeof target.TextEncoder !== 'function') target.TextEncoder = SayItTextEncoder
  if (typeof target.Blob !== 'function') target.Blob = SayItBlob
  if (typeof target.FormData !== 'function') target.FormData = SayItFormData
  if (typeof target.URL !== 'function') target.URL = SayItURL
  target.__SayItReadableStream = SayItReadableStream
})()
