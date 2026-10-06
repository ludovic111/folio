; folio's Windows installer (scripts/bundle-windows.sh runs makensis on it):
;   makensis /DVERSION=0.1.0 /DSRC=<folder with folio.exe…> /DOUTFILE=<setup.exe> installer.nsi
;
; Per-user install into %LOCALAPPDATA%\folio, a Start menu shortcut, and .folio files opening in
; folio. `/P` skips the pages (passive, for an updater), `/R` starts folio when done; /S is NSIS's
; silent mode.

Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!define PRODUCT "folio"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT}"
!define PROGID "folio.document"

Name "${PRODUCT}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\${PRODUCT}"
InstallDirRegKey HKCU "${UNINSTKEY}" "InstallLocation"
BrandingText "folio ${VERSION} · lsuite"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "folio installer"
VIAddVersionKey "LegalCopyright" "MIT licence"

!define MUI_ICON "${SRC}\folio.ico"
!define MUI_UNICON "${SRC}\folio.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\folio.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open folio"

Var Passive

!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipWhenPassive
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipWhenPassive
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  StrCpy $Passive 0
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/P" $1
  ${IfNot} ${Errors}
    StrCpy $Passive 1
    SetAutoClose true
  ${EndIf}
FunctionEnd

Function SkipWhenPassive
  ${If} $Passive == 1
    Abort
  ${EndIf}
FunctionEnd

Section "folio" Main
  SetOutPath "$INSTDIR"
  ; folio may still be closing (an updater quits it just before running this).
  Sleep 500
  SetOverwrite on
  File "${SRC}\folio.exe"
  File "${SRC}\folio-cli.exe"
  File "${SRC}\folio-mcp.exe"
  File "${SRC}\folio.ico"
  File "${SRC}\LICENSE.txt"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortCut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\folio.exe" "" "$INSTDIR\folio.ico" 0

  ; .folio documents open in folio; Word, Excel, PowerPoint, OpenDocument, CSV and Markdown files
  ; list it under "Open with".
  WriteRegStr HKCU "Software\Classes\.folio" "" "${PROGID}"
  WriteRegStr HKCU "Software\Classes\.folio" "Content Type" "application/vnd.lsuite.folio"
  WriteRegStr HKCU "Software\Classes\${PROGID}" "" "folio document"
  WriteRegStr HKCU "Software\Classes\${PROGID}\DefaultIcon" "" "$\"$INSTDIR\folio.ico$\""
  WriteRegStr HKCU "Software\Classes\${PROGID}\shell\open\command" "" "$\"$INSTDIR\folio.exe$\" $\"%1$\""
  WriteRegStr HKCU "Software\Classes\Applications\folio.exe\shell\open\command" "" "$\"$INSTDIR\folio.exe$\" $\"%1$\""
  !macro OpenWith ext
    WriteRegStr HKCU "Software\Classes\Applications\folio.exe\SupportedTypes" "${ext}" ""
    WriteRegStr HKCU "Software\Classes\${ext}\OpenWithList\folio.exe" "" ""
  !macroend
  !insertmacro OpenWith ".folio"
  !insertmacro OpenWith ".docx"
  !insertmacro OpenWith ".xlsx"
  !insertmacro OpenWith ".pptx"
  !insertmacro OpenWith ".odt"
  !insertmacro OpenWith ".ods"
  !insertmacro OpenWith ".odp"
  !insertmacro OpenWith ".csv"
  !insertmacro OpenWith ".md"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'

  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\folio.ico$\""
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "lsuite"
  WriteRegStr HKCU "${UNINSTKEY}" "URLInfoAbout" "https://lsuite.xyz/folio"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" "$\"$INSTDIR\uninstall.exe$\" /S"
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" $0

  ; /R (from an updater): start folio again.
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/R" $1
  ${IfNot} ${Errors}
    Exec '"$INSTDIR\folio.exe"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\folio.exe"
  Delete "$INSTDIR\folio-cli.exe"
  Delete "$INSTDIR\folio-mcp.exe"
  Delete "$INSTDIR\folio.ico"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  DeleteRegKey HKCU "${UNINSTKEY}"
  ; The file associations this installer wrote; documents and settings (%APPDATA%\folio) stay.
  ReadRegStr $0 HKCU "Software\Classes\.folio" ""
  ${If} $0 == "${PROGID}"
    DeleteRegKey HKCU "Software\Classes\.folio"
  ${EndIf}
  DeleteRegKey HKCU "Software\Classes\${PROGID}"
  DeleteRegKey HKCU "Software\Classes\Applications\folio.exe"
  !macro UnOpenWith ext
    DeleteRegKey HKCU "Software\Classes\${ext}\OpenWithList\folio.exe"
  !macroend
  !insertmacro UnOpenWith ".docx"
  !insertmacro UnOpenWith ".xlsx"
  !insertmacro UnOpenWith ".pptx"
  !insertmacro UnOpenWith ".odt"
  !insertmacro UnOpenWith ".ods"
  !insertmacro UnOpenWith ".odp"
  !insertmacro UnOpenWith ".csv"
  !insertmacro UnOpenWith ".md"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd
