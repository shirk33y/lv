; lv installer — per-user, no UAC, simple progress bar
; Compile with: makensis installer.nsi

!include "MUI2.nsh"

Name "lv"
OutFile "lv-setup.exe"
InstallDir "$LOCALAPPDATA\lv"
RequestExecutionLevel user
SetCompressor /SOLID lzma

; ── UI ───────────────────────────────────────────────────────────────
!define MUI_ICON "lv.ico"
!define MUI_ABORTWARNING

; Skip welcome/license/directory pages — just install
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; ── Install ──────────────────────────────────────────────────────────
Section "Install"
  SetOutPath "$INSTDIR"

  ; Main binary
  File "lv-imgui.exe"

  ; Runtime DLLs
  File "SDL2.dll"
  File "mpv-2.dll"

  ; Create start menu shortcut
  CreateDirectory "$SMPROGRAMS\lv"
  CreateShortCut "$SMPROGRAMS\lv\lv.lnk" "$INSTDIR\lv-imgui.exe"

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

  ; Add to PATH (user)
  EnVar::AddValue "PATH" "$INSTDIR"

  ; Register file associations (user-level)
  WriteRegStr HKCU "Software\Classes\.jpg\OpenWithProgids" "lv.image" ""
  WriteRegStr HKCU "Software\Classes\.jpeg\OpenWithProgids" "lv.image" ""
  WriteRegStr HKCU "Software\Classes\.png\OpenWithProgids" "lv.image" ""
  WriteRegStr HKCU "Software\Classes\.gif\OpenWithProgids" "lv.image" ""
  WriteRegStr HKCU "Software\Classes\.webp\OpenWithProgids" "lv.image" ""
  WriteRegStr HKCU "Software\Classes\.mp4\OpenWithProgids" "lv.video" ""
  WriteRegStr HKCU "Software\Classes\.mkv\OpenWithProgids" "lv.video" ""
  WriteRegStr HKCU "Software\Classes\lv.image\shell\open\command" "" \
    "$\"$INSTDIR\lv-imgui.exe$\" $\"%1$\""
  WriteRegStr HKCU "Software\Classes\lv.video\shell\open\command" "" \
    "$\"$INSTDIR\lv-imgui.exe$\" $\"%1$\""
SectionEnd

; ── Uninstall ────────────────────────────────────────────────────────
Section "Uninstall"
  Delete "$INSTDIR\lv-imgui.exe"
  Delete "$INSTDIR\SDL2.dll"
  Delete "$INSTDIR\mpv-2.dll"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\lv\lv.lnk"
  RMDir "$SMPROGRAMS\lv"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\lv"
  DeleteRegKey HKCU "Software\Classes\lv.image"
  DeleteRegKey HKCU "Software\Classes\lv.video"

  EnVar::DeleteValue "PATH" "$INSTDIR"
SectionEnd
