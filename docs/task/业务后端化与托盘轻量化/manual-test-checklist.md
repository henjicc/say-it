# 最终人工测试清单

## 执行约定

- 用户决定：所有人工测试统一在全部任务完成后执行，不作为中间任务或阶段门禁。
- 后续任务发现新的鼠标、快捷键、真实音频、窗口、OBS 或性能验证项时，继续追加到本文件。
- 每项执行后记录：通过/失败、测试时间、版本/提交、模型与音源、错误信息或截图；失败须判断是迁移前既有问题还是本次回归。

## 功能回归

- [ ] 实时听写：说一句并停止，最终文本只注入一次；中途取消不注入且指示窗恢复。
- [ ] 文件听写：Fun-ASR-Flash、Qwen3-ASR-Flash 各执行一次且只注入一次；本地规则顺序和替换结果不变。
- [ ] 实时字幕：麦克风与系统音频分别持续滚动；有 OBS 环境时输出同步。
- [ ] 设置页静音测试：成功路径完成且页面状态恢复。
- [ ] 录音识别：真实音频完成转写；纯文本/字幕切换、复制、SRT 导出及播放器时间抽查正常。
- [ ] 文稿对齐：真实音频和逐行文稿执行；两种结果、缓存复用、SRT 文本与时间正常。
- [ ] 字幕编辑器：两个入口均验证 cue 整体/边缘拖动、吸附、缩放、播放头、文本编辑、±100ms 微调，输入框快捷键不误触发。

## 资源与时延

- [ ] 前台稳定 60 秒后执行：`powershell -File scripts/测量进程内存.ps1 -Condition foreground -OutputPath docs/task/业务后端化与托盘轻量化/baseline-foreground.json`。
- [ ] 关闭到托盘后执行：`powershell -File scripts/测量进程内存.ps1 -Condition tray-idle-60s -OutputPath docs/task/业务后端化与托盘轻量化/baseline-tray.json`。
- [ ] 完成一次听写后执行：`powershell -File scripts/测量进程内存.ps1 -Condition after-dictation -OutputPath docs/task/业务后端化与托盘轻量化/baseline-after-dictation.json`。
- [ ] 实时字幕运行中执行：`powershell -File scripts/测量进程内存.ps1 -Condition subtitles-running -OutputPath docs/task/业务后端化与托盘轻量化/baseline-subtitles.json`。
- [ ] 托盘点击到主窗口可交互测 5 次，记录每次毫秒数和中位数。
- [ ] 听写快捷键按下到指示器首次可见测 5 次，记录每次毫秒数和中位数。
- [ ] 最终托盘总工作集相对当前托盘基线下降至少 40%，空闲 CPU 不高于基线，恢复与快捷键响应增量不超过 100ms。
