# 供应商旧 ID 迁移碰撞与加载未去重导致重复配置

## 现象与证据

- 2026-10-29 只读检查状态主文件与备份：11 个 profile 中有 5 个 `id=bailian`、`kind=sdk:bailian`，另外 6 个 id 不重复。四个多余副本排在新内置 `local-ppocr` 之前。
- 使用不含真实凭据的 `src-tauri/src/providers/fixtures/duplicate-profiles.json` 复现同样的数量与顺序。修复前目标测试失败：归一化仍返回全部 11 项，`funasr` 与 `bailian` 并存时迁移后留下 2 个 `bailian`，未知供应商的重复项也被保留。
- 解码本机 WebView localStorage 的两个 SST 文件和 WAL，仅检查存储键和供应商元数据：现存镜像是听写、字幕、对比偏好与主题，没有供应商列表。`plugin-backups` 仅有豆包插件包；现存诊断日志没有供应商迁移轨迹。因此这些材料不能确定本机重复首次出现的日期，也不能证明实际发生过几次版本往返。

## 根因与增长条件

1. `046bae3`（2026-08-28）的 `persistence::migrate_provider_ids` 逐项把 `funasr` 改为 `bailian`，但没有处理目标 id 已存在的情况。多个旧项或新旧项并存都会造成同 id 碰撞。
2. `providers::normalize_settings` 通过 `find` 只更新第一个内置项，未知项原样保留，没有维护 profile id 唯一性；保存也复用此入口，因此损坏会持续落盘。
3. `046bae3^` 的内置 id 是 `funasr`，旧版归一化找不到它时会追加，旧版保存写 schema 2。若先用新版数据运行旧版，再升级，迁移便会把追加项改成另一个 `bailian`。每轮可增加一份，四轮可把一份累积到五份。这是代码可复现的增长机制，不是对用户运行历史的断言。
4. 单独重复执行当前版本的加载、保存、归一化或迁移，不会凭空追加另一个 `bailian`，但修复前也不会减少已有副本。再次输入带旧 id 的数据才会触发新的碰撞。

`c56ff6b` 中 `migrate_provider_ids` 只有凭据迁移这一处生产调用，其余调用位于测试模块；schema 4 的正常加载直接跳过该旧 ID 迁移。不能把搜索结果中的测试调用计为多条生产导入路径，也不能将重复归因于每次启动多次执行改名。

`65d6ad0` 在相关结构上增加 `keepLoaded`，没有改名/去重逻辑；`c56ff6b` 增加内置 PP-OCR、移除 Windows 系统 OCR，也没有引入百炼重复的写入逻辑。缺陷早已存在于上述旧 ID 迁移及归一化实现；数据中的相对顺序与早于此次合并的重复相符，但不能仅凭顺序确定损坏时间。

## 写入边界排查

| 路径 | 行为与去重边界 |
| --- | --- |
| `load_persisted_state`、旧 identifier 文件回退、主文件/备份恢复 | 凭据与 ID 迁移后统一归一化 |
| `save_persisted_state_with_app_settings` | 序列化前统一归一化、拒绝明文凭据 |
| `commands/session.rs` 的默认供应商、配置更新、添加/删除 LLM | 先归一化；配置更新只修改首项；新增 LLM 使用 UUID id |
| `application/llm_models.rs`、`commands/customization.rs` | 基于归一化后的设置修改 |
| 插件初始化、重扫、安装、启停、卸载 | 注册表按 id 查找/追加，合并末尾归一化；注册表加载还会拒绝与内置供应商或已登记供应商的 id 冲突 |
| 前端 `useProviderStore` | 投影后端响应，配置自动保存调用 `update_provider_config`，不上传整份 profile 列表 |
| `import_legacy_settings` 与 localStorage 双写回退镜像 | 当前及 `046bae3` 前的 `LegacySettingsImport` 都仅有应用偏好、主题、提示音字段，无供应商列表；不能把该路径误判为重复追加源 |

## 修复约束

- 同 id 按列表顺序保留第一份完整 profile，保留顺序、非密钥 config、启用状态与 `keepLoaded`。内置元数据及默认项继续按现有归一化规则校正；不从后续副本补字段，避免复活已清空的旧设置。旧 id 改名碰撞也采用同一规则。
- 与运行时 `find_profile`、配置更新的首项语义一致，不在前端过滤掩盖存储损坏。
- 凭据收集/校验必须在迁移去重前执行。内置凭据键由 provider id 与字段构成，插件还有 plugin id 命名空间，不依赖列表下标。已存储的 `bailian` 凭据不因去重被读取、修改或删除；不同旧明文凭据映射到同一键时，原有冲突校验仍会拒绝迁移并回滚。
- 不增加 schema 版本。当前 schema 也会在加载时归一化；第一次正常保存会将干净主文件写回。常规备份保存上一版磁盘内容，第一次备份可能仍含重复，但再加载会同样自愈。
- 路径和凭据库可注入的加载函数供临时目录测试使用，不对真实数据运行会创建锁、恢复备份或写回迁移的加载器。

## 回归防线

`cargo test --manifest-path src-tauri/Cargo.toml --bin say-it provider_id_duplicates` 覆盖：五份百炼、两种别名顺序、当前 schema 加载/保存、多轮旧数据导入、凭据在合并前提取、未知供应商、有效/失效 defaults、模型包注册表合并、配置与 `keepLoaded` 保留及幂等性。使用临时文件与 FakeCredentialStore，不读取实际凭据或访问云端服务。

跨平台夹具的预期必须计入归一化会补入的内置供应商。该夹具去重后有 7 个 id；macOS 还会在末尾追加缺失的 `system-ocr`（Vision），总计 8 个，Windows 则为 7 个。数量断言按 `builtin_profiles().len() + 1`（一个额外模型包）计算，顺序断言显式列出 macOS 追加项，不仅检查重复项数量。`check:mac-overlay` 只检查 `sayit-overlay-macos` 子 crate，不能代替主包供应商与持久化测试在 macOS 上运行。
