; GPL-3.0-only. Per-user installation; no elevation or input-device permissions.
Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"

!ifndef PAYLOAD
  !error "PAYLOAD must name the verified portable package directory"
!endif
!ifndef OUTPUT
  !error "OUTPUT must name the installer executable"
!endif
!ifndef VERSION
  !error "VERSION must match the workspace version"
!endif
!ifndef UNINSTALL_FILES
  !error "UNINSTALL_FILES must name the generated exact installed-file manifest"
!endif

Name "C64 Keyboard Bridge"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\C64 Keyboard Bridge"
InstallDirRegKey HKCU "Software\C64KeyboardBridge\Installer" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID zlib
ShowInstDetails show
ShowUninstDetails show
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "C64 Keyboard Bridge"
VIAddVersionKey "FileDescription" "C64 Keyboard Bridge per-user installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "GPL-3.0-only; C64 Keyboard Bridge contributors"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${PAYLOAD}\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
; Deliberately no launch option: a fresh install never starts hosting or input.
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "This installer requires 64-bit Windows."
    Abort
  ${EndIf}
  SetShellVarContext current
FunctionEnd

Section "C64 Keyboard Bridge" SEC_MAIN
  SetOutPath "$INSTDIR"
  File /r "${PAYLOAD}\*.*"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateDirectory "$SMPROGRAMS\C64 Keyboard Bridge"
  CreateShortcut "$SMPROGRAMS\C64 Keyboard Bridge\C64 Keyboard Bridge.lnk" "$INSTDIR\c64bridge-gui.exe"
  CreateShortcut "$SMPROGRAMS\C64 Keyboard Bridge\Uninstall.lnk" "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\C64KeyboardBridge\Installer" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\C64KeyboardBridge" "DisplayName" "C64 Keyboard Bridge"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\C64KeyboardBridge" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\C64KeyboardBridge" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\C64KeyboardBridge" "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\C64KeyboardBridge" "QuietUninstallString" '$\"$INSTDIR\uninstall.exe$\" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\C64KeyboardBridge" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\C64KeyboardBridge" "NoRepair" 1
SectionEnd

Function un.onInit
  SetShellVarContext current
FunctionEnd

Section "Uninstall"
  ; Application settings and OS-vault credentials live outside this directory.
  Delete "$SMPROGRAMS\C64 Keyboard Bridge\C64 Keyboard Bridge.lnk"
  Delete "$SMPROGRAMS\C64 Keyboard Bridge\Uninstall.lnk"
  RMDir "$SMPROGRAMS\C64 Keyboard Bridge"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\C64KeyboardBridge"
  DeleteRegKey HKCU "Software\C64KeyboardBridge\Installer"
  !include "${UNINSTALL_FILES}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
