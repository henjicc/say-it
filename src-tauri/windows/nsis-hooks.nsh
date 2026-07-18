!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $R0 SHCTX "Software\Classes\.sayit" ""
  ${If} $R0 != ""
    WriteRegStr SHCTX "Software\Classes\$R0\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  ${EndIf}
  !insertmacro UPDATEFILEASSOC
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro UPDATEFILEASSOC
!macroend
