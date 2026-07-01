# Tauri 图标源图非正方形生成失败处理

## 触发场景

执行 `npx tauri icon <图片路径>` 时，如果源图不是正方形，会报错：

```text
Source image must be square
```

## 处理方式

保留原图完整内容，先用透明背景补成正方形，再把补齐后的临时图交给 Tauri 生成全套图标：

```powershell
magick "docs\images\语音输入.png" -background none -gravity center -extent 1431x1431 "$env:TEMP\sayit-icon-square.png"
npx tauri icon "$env:TEMP\sayit-icon-square.png"
```

`-extent` 的尺寸取原图宽高中的较大值，避免裁掉图标内容。
