# 修改 Tauri identifier 时要兼容旧本地状态目录

## 触发条件

- 修改 `src-tauri/tauri.conf.json` 里的 `identifier`，例如从旧包名切到新包名。
- 本地状态文件通过 `app.path().app_local_data_dir()` 存储。

## 正确做法

- 不能只改 `identifier`，否则应用会切到新的本地数据目录。
- 如果热键、启动项、供应商设置等依赖本地状态文件，启动时需要兼容读取旧 `identifier` 目录下的状态文件。
- 最小做法是在 `load_persisted_state()` 中，当新目录不存在状态文件时，按已知旧 `identifier` 列表回退查找并加载。
- 后续用户再次保存设置时，再自然写入新目录即可，不需要在迁移首帧强制复制文件。

## 这次场景

- `identifier` 从 `com.vibecode.sayit` 改为 `com.henjicc.sayit`。
- 如果不兼容旧目录，应用会读不到原来的 `say-it-state.json`，表现为快捷键、启动设置等像“失效”一样回到默认状态。
