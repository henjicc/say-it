import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import ts from 'typescript'

const projectRoot = path.resolve(import.meta.dirname, '..')
const output = process.argv[2]
if (!output) throw new Error('缺少 SDK Runtime bootstrap 输出路径')

const sources = ['web-compat.ts', 'host-adapter.ts'].map((name) => {
  const file = path.join(projectRoot, 'sdk-runtime', name)
  const result = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
    compilerOptions: {
      target: ts.ScriptTarget.ES2021,
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

fs.mkdirSync(path.dirname(output), { recursive: true })
fs.writeFileSync(output, sources.join('\n'))
