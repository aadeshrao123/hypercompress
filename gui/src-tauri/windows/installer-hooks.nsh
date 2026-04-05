; HyperCompress NSIS installer hooks
; Registers Explorer context menu entries on install, removes on uninstall

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\*\shell\HyperCompress" "" "Compress with HyperCompress"
  WriteRegStr SHCTX "Software\Classes\*\shell\HyperCompress" "Icon" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr SHCTX "Software\Classes\*\shell\HyperCompress\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" compress "%1"'

  WriteRegStr SHCTX "Software\Classes\Directory\shell\HyperCompress" "" "Compress with HyperCompress"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\HyperCompress" "Icon" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr SHCTX "Software\Classes\Directory\shell\HyperCompress\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" compress "%1"'

  WriteRegStr SHCTX "Software\Classes\Directory\Background\shell\HyperCompress" "" "Compress with HyperCompress"
  WriteRegStr SHCTX "Software\Classes\Directory\Background\shell\HyperCompress" "Icon" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr SHCTX "Software\Classes\Directory\Background\shell\HyperCompress\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" compress "%V"'

  WriteRegStr SHCTX "Software\Classes\.hc" "" "HyperCompress.Archive"
  WriteRegStr SHCTX "Software\Classes\HyperCompress.Archive" "" "HyperCompress Archive"
  WriteRegStr SHCTX "Software\Classes\HyperCompress.Archive\DefaultIcon" "" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr SHCTX "Software\Classes\HyperCompress.Archive\shell" "" "extract"
  WriteRegStr SHCTX "Software\Classes\HyperCompress.Archive\shell\extract" "" "Extract with HyperCompress"
  WriteRegStr SHCTX "Software\Classes\HyperCompress.Archive\shell\extract" "Icon" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr SHCTX "Software\Classes\HyperCompress.Archive\shell\extract\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" decompress "%1"'
  WriteRegStr SHCTX "Software\Classes\HyperCompress.Archive\shell\open" "" "Open with HyperCompress"
  WriteRegStr SHCTX "Software\Classes\HyperCompress.Archive\shell\open\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" decompress "%1"'

  System::Call 'Shell32::SHChangeNotify(i 0x8000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey SHCTX "Software\Classes\*\shell\HyperCompress"
  DeleteRegKey SHCTX "Software\Classes\Directory\shell\HyperCompress"
  DeleteRegKey SHCTX "Software\Classes\Directory\Background\shell\HyperCompress"
  DeleteRegKey SHCTX "Software\Classes\HyperCompress.Archive"
  DeleteRegKey SHCTX "Software\Classes\.hc"

  System::Call 'Shell32::SHChangeNotify(i 0x8000000, i 0, p 0, p 0)'
!macroend
