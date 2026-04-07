; HyperCompress NSIS installer hooks.
;
; Modern Win11 top-level context menu via sparse-packaged IExplorerCommand.
; Per-machine install required so we can write to LocalMachine cert stores
; (Add-AppxPackage validates the signing cert against LocalMachine\TrustedPeople
; and the chain via LocalMachine\Root — no other store works for sideloaded MSIX).
;
; Legacy registry verbs are kept ONLY as a Win10 fallback. They're stripped from
; the .hc file class to avoid duplicate flat entries on Win11 alongside our
; cascading modern menu.

!macro NSIS_HOOK_POSTINSTALL
  ; ---- legacy fallback (Win10 / "Show more options") ----
  WriteRegStr SHCTX "Software\Classes\*\shell\HyperCompress" "" "Compress with HyperCompress"
  WriteRegStr SHCTX "Software\Classes\*\shell\HyperCompress" "Icon" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr SHCTX "Software\Classes\*\shell\HyperCompress\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" compress "%1"'

  WriteRegStr SHCTX "Software\Classes\Directory\shell\HyperCompress" "" "Compress with HyperCompress"
  WriteRegStr SHCTX "Software\Classes\Directory\shell\HyperCompress" "Icon" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr SHCTX "Software\Classes\Directory\shell\HyperCompress\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" compress "%1"'

  WriteRegStr SHCTX "Software\Classes\Directory\Background\shell\HyperCompress" "" "Compress with HyperCompress"
  WriteRegStr SHCTX "Software\Classes\Directory\Background\shell\HyperCompress" "Icon" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr SHCTX "Software\Classes\Directory\Background\shell\HyperCompress\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" compress "%V"'

  ; .hc file association: icon + double-click handler only (no shell verbs)
  WriteRegStr SHCTX "Software\Classes\.hc" "" "HyperCompress.Archive"
  WriteRegStr SHCTX "Software\Classes\HyperCompress.Archive" "" "HyperCompress Archive"
  WriteRegStr SHCTX "Software\Classes\HyperCompress.Archive\DefaultIcon" "" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr SHCTX "Software\Classes\HyperCompress.Archive\shell\open\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" decompress "%1"'
  DeleteRegKey SHCTX "Software\Classes\HyperCompress.Archive\shell\extract"

  ; ---- modern Win11 sparse package registration ----
  StrCpy $0 ""
  IfFileExists "$INSTDIR\shellext\HCShellExt.msix" 0 +3
    StrCpy $0 "$INSTDIR"
    Goto hc_have_pkg
  IfFileExists "$INSTDIR\resources\shellext\HCShellExt.msix" 0 +3
    StrCpy $0 "$INSTDIR\resources"
    Goto hc_have_pkg
  Goto hc_skip_modern

  hc_have_pkg:
  ; Run the install script. It logs to $0\shellext\install.log so we can debug failures.
  nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -File "$0\shellext\install-shellext.ps1" -InstallDir "$0"'

  hc_skip_modern:

  System::Call 'Shell32::SHChangeNotify(i 0x8000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; ---- remove modern sparse package and cert ----
  IfFileExists "$INSTDIR\shellext\uninstall-shellext.ps1" 0 +3
    nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\shellext\uninstall-shellext.ps1"'
    Goto hc_unins_done
  IfFileExists "$INSTDIR\resources\shellext\uninstall-shellext.ps1" 0 hc_unins_done
    nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\resources\shellext\uninstall-shellext.ps1"'
  hc_unins_done:

  ; ---- remove legacy entries ----
  DeleteRegKey SHCTX "Software\Classes\*\shell\HyperCompress"
  DeleteRegKey SHCTX "Software\Classes\Directory\shell\HyperCompress"
  DeleteRegKey SHCTX "Software\Classes\Directory\Background\shell\HyperCompress"
  DeleteRegKey SHCTX "Software\Classes\HyperCompress.Archive"
  DeleteRegKey SHCTX "Software\Classes\.hc"

  System::Call 'Shell32::SHChangeNotify(i 0x8000000, i 0, p 0, p 0)'
!macroend
