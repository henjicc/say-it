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
const expectedSdkVersion = '0.12.0'
const expectedSdkTarball = 'https://registry.npmjs.org/@henjicc/ai-sdk/-/ai-sdk-0.12.0.tgz'
const expectedSdkShasum = 'da64351ba6c8a3fc932c1cef00b54f23e3a4446d'
const expectedSdkIntegrity = 'sha512-/FBxWZM2SYmulbc9Y7vGTLaqVd1pkP00vHWTa9Fz18rjpQTHt0Q6AfTbpo49mjRzu/846Q+KM50oezV4BCmB6Q=='
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
const groq = await bundleSdkEntry({
  entry: 'groq-entry.ts',
  globalName: '__sayitAiSdkGroq',
  output: 'sayit-ai-sdk-groq.js',
  forbiddenInputs: [
    '/dist/generation',
    '/dist/catalog/',
    '/dist/capabilities/',
    '/dist/upload/',
    '/dist/providers/endpoints/',
    '/dist/providers/ppio-media',
    '/dist/llm/bigmodel/',
  ],
  plugins: [groqBundleBoundaryPlugin()],
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
// 供应商预设里走 Responses API 的模型；不在表里的模型走 Chat Completions。
const llmResponsesModels = Object.fromEntries(
  LLM_PROVIDER_PRESETS
    .filter(preset => preset.responsesModelIds)
    .map(preset => [preset.providerId, preset.responsesModelIds])
)

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
    groq: ['groq-llm'],
    llmModules: ['plugin-llm'],
  },
  bundles: {
    bridge: describeFile(bridgePath, 2),
    capabilities,
    groq,
    llmModules,
    bootstrap: describeFile(sdkBootstrapPath, 1),
  },
  pricing,
  llmResponsesModels,
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

function groqBundleBoundaryPlugin() {
  const preprocessReplacement = path.join(projectRoot, 'sdk-runtime', 'groq-text-only-preprocess.ts')
  const endpointIdentityReplacement = path.join(projectRoot, 'sdk-runtime', 'groq-endpoint-identity.ts')
  return {
    name: 'sayit-groq-bundle-boundary',
    setup(buildContext) {
      buildContext.onResolve({ filter: /^\.\.\/upload\/preprocess\.js$/ }, args => {
        const importer = args.importer.replaceAll('\\', '/')
        if (!importer.endsWith('/dist/llm/chat.js')) return undefined
        return { path: preprocessReplacement }
      })
      buildContext.onResolve({ filter: /^\.\/endpointProfiles\.js$/ }, args => {
        const importer = args.importer.replaceAll('\\', '/')
        if (!importer.includes('/dist/llm/')) return undefined
        return { path: endpointIdentityReplacement }
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
