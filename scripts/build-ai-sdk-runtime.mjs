import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'
import { build } from 'esbuild'
import ts from 'typescript'

const projectRoot = path.resolve(import.meta.dirname, '..')
const outputDirectory = process.argv[2]
if (!outputDirectory) throw new Error('缺少 SDK Runtime 输出目录')

const packageManifest = readJson(path.join(projectRoot, 'package.json'))
const packageLock = readJson(path.join(projectRoot, 'package-lock.json'))
const sdkVersion = packageManifest.dependencies?.['@henjicc/ai-sdk']
const sdkLock = packageLock.packages?.['node_modules/@henjicc/ai-sdk']
const expectedSdkVersion = '0.15.0'
const expectedSdkTarball = 'https://registry.npmjs.org/@henjicc/ai-sdk/-/ai-sdk-0.15.0.tgz'
const expectedSdkShasum = 'bbc46b0d1db83e4ba2fb6d125b735c00fda28bed'
const expectedSdkIntegrity = 'sha512-6+L0+oNhGm04qtrjnfrNJOL2HJa6eD4ooumIvPf8yCYo8+2rtp4N9eBDrpFds7BpWSqEbZxdC4+liP30TwJgVA=='
if (sdkVersion !== expectedSdkVersion || sdkLock?.version !== sdkVersion) {
  throw new Error(`Say-It 必须精确锁定 @henjicc/ai-sdk@${expectedSdkVersion}`)
}
if (
  sdkLock.integrity !== expectedSdkIntegrity
  || sdkLock.resolved !== expectedSdkTarball
) {
  throw new Error('AI SDK lock 与公共 npm 发布产物不一致')
}
if (packageManifest.devDependencies?.esbuild !== '0.25.12') {
  throw new Error('SDK Runtime bundler 必须精确锁定 esbuild@0.25.12')
}

const outputRoot = path.resolve(projectRoot, outputDirectory)
fs.mkdirSync(outputRoot, { recursive: true })

const bridge = transpileScripts(['web-compat.ts', 'host-adapter.ts']).join('\n')
const sdkBootstrap = transpileScripts(['sdk-bootstrap.ts'])[0]
  .replaceAll('__SAYIT_AI_SDK_VERSION__', sdkVersion)
const bridgePath = path.join(outputRoot, 'sayit-sdk-runtime-bootstrap.js')
const sdkBootstrapPath = path.join(outputRoot, 'sayit-ai-sdk-bootstrap.js')
fs.writeFileSync(bridgePath, bridge)
fs.writeFileSync(sdkBootstrapPath, sdkBootstrap)

const capabilities = await bundleSdkEntry({
  entry: 'capability-entry.ts',
  globalName: '__sayitAiSdkCapabilities',
  output: 'sayit-ai-sdk-capabilities.js',
  forbiddenInputs: ['/dist/generation', '/dist/catalog/', '/dist/llm/'],
})
const llm = await bundleSdkEntry({
  entry: 'llm-entry.ts',
  globalName: '__sayitAiSdkLlm',
  output: 'sayit-ai-sdk-llm.js',
  forbiddenInputs: [
    '/dist/generation',
    '/dist/catalog/',
    '/dist/capabilities/',
    '/dist/upload/',
    '/dist/providers/endpoints/',
    '/dist/providers/ppio-media',
  ],
  plugins: [textOnlyPreprocessPlugin()],
})
const llmModules = await bundleSdkEntry({
  entry: 'llm-modules-entry.ts',
  globalName: '__sayitAiSdkLlmModules',
  output: 'sayit-ai-sdk-llm-modules.js',
  forbiddenInputs: [
    '/dist/generation',
    '/dist/catalog/',
    '/dist/capabilities/speech-recognition/',
    '/dist/capabilities/translation/',
    '/dist/capabilities/realtime',
    '/dist/capabilities/client',
    '/dist/capabilities/builtin-descriptors',
    '/dist/providers/',
    '/dist/llm/bigmodel/',
  ],
})

const { LLM_PROVIDER_PRESETS } = await import('@henjicc/ai-sdk/llm')
const pricing = await collectPricing()
const llmHosts = await collectLlmHosts()

const manifest = {
  sdk: {
    package: '@henjicc/ai-sdk',
    version: sdkVersion,
    resolved: sdkLock.resolved,
    integrity: sdkLock.integrity,
    shasum: expectedSdkShasum,
    sourceNamespace: '@henjicc/ai-sdk',
  },
  moduleSources: {
    capabilities: [
      'bailian-speech-recognition',
      'bailian-speech-recognition-realtime',
      'bailian-translation',
      'volcengine-speech-recognition',
      'volcengine-speech-recognition-realtime',
      'siliconflow-speech-recognition',
      'groq-speech-recognition',
    ],
    llm: ['llm'],
    llmModules: ['plugin-llm'],
  },
  bundles: {
    bridge: describeFile(bridgePath, 2),
    capabilities,
    llm,
    llmModules,
    bootstrap: describeFile(sdkBootstrapPath, 1),
  },
  pricing,
  llmHosts,
}
fs.writeFileSync(
  path.join(outputRoot, 'sayit-ai-sdk-manifest.json'),
  `${JSON.stringify(manifest, null, 2)}\n`
)
console.log(JSON.stringify(manifest))

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'))
}

function transpileScripts(names) {
  return names.map(name => {
    const file = path.join(projectRoot, 'sdk-runtime', name)
    const result = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
      compilerOptions: {
        target: ts.ScriptTarget.ES2020,
        module: ts.ModuleKind.None,
        removeComments: false,
      },
      fileName: file,
      reportDiagnostics: true,
    })
    if (result.diagnostics?.length) {
      const message = ts.formatDiagnosticsWithColorAndContext(result.diagnostics, {
        getCanonicalFileName: value => value,
        getCurrentDirectory: () => projectRoot,
        getNewLine: () => '\n',
      })
      throw new Error(message)
    }
    return result.outputText
  })
}

async function bundleSdkEntry({ entry, globalName, output, forbiddenInputs, plugins = [] }) {
  const outputPath = path.join(outputRoot, output)
  const result = await build({
    absWorkingDir: projectRoot,
    entryPoints: [path.join('sdk-runtime', entry)],
    outfile: outputPath,
    bundle: true,
    format: 'iife',
    globalName,
    platform: 'browser',
    target: 'es2020',
    treeShaking: true,
    minify: true,
    legalComments: 'none',
    sourcemap: false,
    metafile: true,
    plugins,
    logLevel: 'silent',
  })
  const inputs = Object.keys(result.metafile.inputs).map(value => value.replaceAll('\\', '/'))
  const forbidden = inputs.filter(input => forbiddenInputs.some(marker => input.includes(marker)))
  if (forbidden.length > 0) {
    throw new Error(`${entry} 引入越界模块：${forbidden.join(', ')}`)
  }
  const source = fs.readFileSync(outputPath, 'utf8')
  if (/\b(?:require\s*\(|process\.|globalThis\.fetch|window\.|document\.)/.test(source)) {
    throw new Error(`${entry} 产物包含 Node/WebView 或全局 fetch 依赖`)
  }
  return describeFile(outputPath, inputs.length)
}

/**
 * SDK 登记的官方按量标准价，供模型目录下发给界面显示。能力价格取自运行时实际注册的模块
 * （与 capability-entry 同一份来源列表），大模型价格取自 SDK 的供应商预设。
 */
async function collectPricing() {
  const probePath = path.join(outputRoot, 'sayit-pricing-probe.mjs')
  await build({
    absWorkingDir: projectRoot,
    entryPoints: [path.join('sdk-runtime', 'capability-entry.ts')],
    outfile: probePath,
    bundle: true,
    format: 'esm',
    platform: 'node',
    target: 'es2020',
    logLevel: 'silent',
  })
  let entry
  try {
    entry = await import(pathToFileURL(probePath).href)
  } finally {
    fs.rmSync(probePath)
  }
  const runtime = entry.createSayItCapabilityRuntime({}, { sources: entry.SAYIT_CAPABILITY_MODULE_SOURCES })
  const capabilities = Object.fromEntries(
    runtime.descriptors()
      .filter(descriptor => descriptor.pricing)
      .map(descriptor => [descriptor.id, descriptor.pricing])
  )
  const llm = Object.fromEntries(
    LLM_PROVIDER_PRESETS
      .filter(preset => preset.modelPricing)
      .map(preset => [preset.providerId, preset.modelPricing])
  )
  return { capabilities, llm }
}

/**
 * 内置大模型运行时访问的主机，按 SDK 供应商预设 ID，作为 QuickJS 网络白名单。用 SDK 自己的地址解析
 * 函数算出默认配置下 Chat 与 Responses 的实际地址；SDK 只在包内导出这两个函数，构建时按文件路径导入。
 * 预设之外再加 OpenAI：SDK 对它有默认地址但没有预设。
 */
async function collectLlmHosts() {
  const endpoints = await import(pathToFileURL(path.join(
    projectRoot, 'node_modules', '@henjicc', 'ai-sdk', 'dist', 'llm', 'protocolEndpoints.js'
  )).href)
  const providerIds = [...LLM_PROVIDER_PRESETS.map(preset => preset.providerId), 'openai']
  return Object.fromEntries(providerIds.map(providerId => {
    const input = { providerId, modelId: 'host-probe', messages: [] }
    const urls = [
      endpoints.resolveLlmChatCompletionsEndpoint(input),
      endpoints.resolveLlmResponsesEndpoint(input),
    ]
    return [providerId, [...new Set(urls.map(url => new URL(url).host))]]
  }))
}

/**
 * 内置大模型只发文本。SDK 轻量入口会静态带入 generation 媒体上传策略，构建时换成只放行文本的
 * 预处理（`text-only-preprocess.ts`），不让媒体上传与 generation 凭据进入 QuickJS。
 */
function textOnlyPreprocessPlugin() {
  const preprocessReplacement = path.join(projectRoot, 'sdk-runtime', 'text-only-preprocess.ts')
  return {
    name: 'sayit-llm-text-only-preprocess',
    setup(buildContext) {
      buildContext.onResolve({ filter: /^\.\.\/upload\/preprocess\.js$/ }, args => {
        const importer = args.importer.replaceAll('\\', '/')
        if (!importer.endsWith('/dist/llm/chat.js')) return undefined
        return { path: preprocessReplacement }
      })
    },
  }
}

function describeFile(file, modules) {
  const bytes = fs.readFileSync(file)
  return {
    file: path.basename(file),
    bytes: bytes.byteLength,
    modules,
    sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
  }
}
