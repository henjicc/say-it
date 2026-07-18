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
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\onnxruntime.dll"
  Delete "$INSTDIR\onnxruntime_providers_shared.dll"
  Delete "$INSTDIR\sherpa-onnx-c-api.dll"
  Delete "$INSTDIR\sherpa-onnx-cxx-api.dll"
!macroend
