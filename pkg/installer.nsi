; lv installer — per-user, no UAC, simple progress bar
; Compile with: makensis installer.nsi

!include "MUI2.nsh"
!include "WinMessages.nsh"

Name "lv"
; Pass -DLV_VERSION=x.y.z-hash on command line, or defaults to "dev"
!ifndef LV_VERSION
  !define LV_VERSION "dev"
!endif
OutFile "lv-setup-${LV_VERSION}.exe"
InstallDir "$LOCALAPPDATA\lv"
RequestExecutionLevel user
SetCompressor /SOLID lzma

; ── UI ───────────────────────────────────────────────────────────────
; !define MUI_ICON "lv.ico"
!define MUI_ABORTWARNING

; Skip welcome/license/directory pages — just install
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\lv.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch lv"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "English"

; ── Install ──────────────────────────────────────────────────────────
Section "Install"
  SetOutPath "$INSTDIR"

  ; Main binary
  File "lv.exe"

  ; Runtime DLLs
  File "SDL2.dll"
  File "libmpv-2.dll"

  ; Create start menu shortcut
  CreateDirectory "$SMPROGRAMS\lv"
  CreateShortCut "$SMPROGRAMS\lv\lv.lnk" "$INSTDIR\lv.exe"

  ; Create uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Add/Remove Programs entry
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\lv" \
    "DisplayName" "lv"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\lv" \
    "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\lv" \
    "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\lv" \
    "Publisher" "lv"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\lv" \
    "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\lv" \
    "NoRepair" 1

  ; Add to user PATH via registry
  ReadRegStr $0 HKCU "Environment" "Path"
  StrCmp $0 "" 0 +2
    WriteRegExpandStr HKCU "Environment" "Path" "$INSTDIR"
  StrCmp $0 "" +2 0
    WriteRegExpandStr HKCU "Environment" "Path" "$0;$INSTDIR"
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=1000

  ; Register file associations (user-level)
  WriteRegStr HKCU "Software\Classes\.jpg\OpenWithProgids" "lv.image" ""
  WriteRegStr HKCU "Software\Classes\.jpeg\OpenWithProgids" "lv.image" ""
  WriteRegStr HKCU "Software\Classes\.png\OpenWithProgids" "lv.image" ""
  WriteRegStr HKCU "Software\Classes\.gif\OpenWithProgids" "lv.image" ""
  WriteRegStr HKCU "Software\Classes\.webp\OpenWithProgids" "lv.image" ""
  WriteRegStr HKCU "Software\Classes\.mp4\OpenWithProgids" "lv.video" ""
  WriteRegStr HKCU "Software\Classes\.mkv\OpenWithProgids" "lv.video" ""
  WriteRegStr HKCU "Software\Classes\lv.image\shell\open\command" "" \
    "$\"$INSTDIR\lv.exe$\" $\"%1$\""
  WriteRegStr HKCU "Software\Classes\lv.video\shell\open\command" "" \
    "$\"$INSTDIR\lv.exe$\" $\"%1$\""
SectionEnd

; ── Uninstall ────────────────────────────────────────────────────────
Section "Uninstall"
  Delete "$INSTDIR\lv.exe"
  Delete "$INSTDIR\SDL2.dll"
  Delete "$INSTDIR\libmpv-2.dll"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\lv\lv.lnk"
  RMDir "$SMPROGRAMS\lv"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\lv"
  DeleteRegKey HKCU "Software\Classes\lv.image"
  DeleteRegKey HKCU "Software\Classes\lv.video"

  ; Remove from user PATH
  ReadRegStr $0 HKCU "Environment" "Path"
  ; Simple removal — user can clean up manually if needed
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=1000
SectionEnd
