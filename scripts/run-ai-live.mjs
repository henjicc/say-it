import { spawnSync } from 'node:child_process'
import { statSync } from 'node:fs'

if (process.env.SAY_IT_ALLOW_PAID !== '1') {
  throw new Error('真实付费验收默认禁用；必须显式设置 SAY_IT_ALLOW_PAID=1')
}
if (process.env.SAY_IT_PAID_CONFIRM !== '9.18') {
  throw new Error('真实付费验收缺少第二道确认：SAY_IT_PAID_CONFIRM=9.18')
}
if (!process.env.SAY_IT_LIVE_AUDIO || !statSync(process.env.SAY_IT_LIVE_AUDIO).isFile()) {
  throw new Error('真实付费验收需要 SAY_IT_LIVE_AUDIO 指向临时语音文件')
}

const result = spawnSync(
  'cargo',
  [
    'test',
    '--manifest-path',
    'src-tauri/Cargo.toml',
    'providers::sdk_runtime::live_tests::paid_provider_minimum_acceptance',
    '--',
    '--ignored',
    '--exact',
    '--nocapture',
    '--test-threads=1',
  ],
  { cwd: process.cwd(), env: process.env, stdio: 'inherit' },
)

if (result.error) throw result.error
process.exitCode = result.status ?? 1
