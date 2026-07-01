; 产品名 PRODUCTNAME（中文）用于开始菜单/桌面快捷方式/卸载信息等展示文本。
; 若用户在安装向导中直接采用了默认路径（由 PRODUCTNAME 拼出，含中文），
; 这里把安装目录名替换成英文的 MAINBINARYNAME，避免路径出现中文字符；
; 若用户手动改过安装路径，则尊重用户选择、不做替换。
!macro NSIS_HOOK_PREINSTALL
  !if "${INSTALLMODE}" == "currentUser"
    ${If} $INSTDIR == "$LOCALAPPDATA\${PRODUCTNAME}"
      StrCpy $INSTDIR "$LOCALAPPDATA\${MAINBINARYNAME}"
      SetOutPath $INSTDIR
    ${EndIf}
  !endif
!macroend
