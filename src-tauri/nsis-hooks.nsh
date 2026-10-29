; 本文件是 Windows 安装器**唯一**的钩子入口。
;
; NSIS 的宏名全局唯一，同名宏不能定义两次，因此所有 NSIS_HOOK_* 必须集中在这里。
; 曾经还有一份 src-tauri/windows/nsis-hooks.nsh 由 tauri.conf.json 声明，但
; tauri.windows.conf.json 的 installerHooks 是整体替换而非合并，Windows 构建（唯一的
; 打包目标）只会加载本文件，那一份里的 .sayit 图标注册与 UPDATEFILEASSOC 从未执行过。
; 新增钩子请直接加进下面对应的宏，不要另建文件。

; 产品名 PRODUCTNAME（中文）用于开始菜单/桌面快捷方式/卸载信息等展示文本。
; 若用户在安装向导中直接采用了默认路径（由 PRODUCTNAME 拼出，含中文），
; 这里把安装目录名替换成固定英文目录 say-it，避免路径出现中文字符；
; 若用户手动改过安装路径，则尊重用户选择、不做替换。
!macro NSIS_HOOK_PREINSTALL
  Delete "$INSTDIR\onnxruntime.dll"
  Delete "$INSTDIR\onnxruntime_providers_shared.dll"
  Delete "$INSTDIR\sherpa-onnx-c-api.dll"
  Delete "$INSTDIR\sherpa-onnx-cxx-api.dll"
  !if "${INSTALLMODE}" == "currentUser"
    ${If} $INSTDIR == "$LOCALAPPDATA\${PRODUCTNAME}"
    ${OrIf} $INSTDIR == "$LOCALAPPDATA\${MAINBINARYNAME}"
      StrCpy $INSTDIR "$LOCALAPPDATA\say-it"
      SetOutPath $INSTDIR
    ${EndIf}
  !endif
!macroend

!macro NSIS_HOOK_POSTINSTALL
  CopyFiles /SILENT "$INSTDIR\target\release\onnxruntime.dll" "$INSTDIR\onnxruntime.dll"
  CopyFiles /SILENT "$INSTDIR\target\release\onnxruntime_providers_shared.dll" "$INSTDIR\onnxruntime_providers_shared.dll"
  CopyFiles /SILENT "$INSTDIR\target\release\sherpa-onnx-c-api.dll" "$INSTDIR\sherpa-onnx-c-api.dll"
  CopyFiles /SILENT "$INSTDIR\target\release\sherpa-onnx-cxx-api.dll" "$INSTDIR\sherpa-onnx-cxx-api.dll"
  Delete "$INSTDIR\target\release\onnxruntime.dll"
  Delete "$INSTDIR\target\release\onnxruntime_providers_shared.dll"
  Delete "$INSTDIR\target\release\sherpa-onnx-c-api.dll"
  Delete "$INSTDIR\target\release\sherpa-onnx-cxx-api.dll"
  RMDir "$INSTDIR\target\release"
  RMDir "$INSTDIR\target"

  ; Tauri 的 fileAssociations 只写了 .sayit 的 ProgID，没写 DefaultIcon，
  ; 于是资源管理器里 .sayit 显示为白板图标。补上并通知外壳刷新关联缓存，
  ; 否则新装/升级后要等系统自己过期才生效。
  ReadRegStr $R0 SHCTX "Software\Classes\.sayit" ""
  ${If} $R0 != ""
    WriteRegStr SHCTX "Software\Classes\$R0\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  ${EndIf}
  !insertmacro UPDATEFILEASSOC
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\onnxruntime.dll"
  Delete "$INSTDIR\onnxruntime_providers_shared.dll"
  Delete "$INSTDIR\sherpa-onnx-c-api.dll"
  Delete "$INSTDIR\sherpa-onnx-cxx-api.dll"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; 关联已被卸载器移除，通知外壳刷新，避免残留的旧图标与打开方式。
  !insertmacro UPDATEFILEASSOC
!macroend
