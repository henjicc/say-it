# CI 用：transcribe.cpp 的 Vulkan 后端在构建时用 SDK 里的 glslc 编译着色器。
# 只把 SDK 复制到 runner 的临时目录，不改系统设置；本机开发按官方安装器装一次即可。
$ErrorActionPreference = "Stop"
$version = "1.4.363.1"
$installer = Join-Path $env:RUNNER_TEMP "vulkansdk.exe"
$root = Join-Path $env:RUNNER_TEMP "VulkanSDK"
Invoke-WebRequest "https://sdk.lunarg.com/sdk/download/$version/windows/vulkansdk-windows-X64-$version.exe" -OutFile $installer
$install = Start-Process $installer -Wait -PassThru -ArgumentList "--root", $root, "--accept-licenses", "--default-answer", "--confirm-command", "install", "copy_only=1"
if ($install.ExitCode -ne 0) { throw "Vulkan SDK 安装失败：$($install.ExitCode)" }
if (-not (Test-Path (Join-Path $root "Bin/glslc.exe"))) { throw "Vulkan SDK 里没有 glslc" }
"VULKAN_SDK=$root" >> $env:GITHUB_ENV
