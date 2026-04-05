Unicode true

!include "MUI2.nsh"

!ifndef PAYLOAD_DIR
  !error "PAYLOAD_DIR is required. Invoke with /DPAYLOAD_DIR=..."
!endif

!ifndef OUTPUT_DIR
  !error "OUTPUT_DIR is required. Invoke with /DOUTPUT_DIR=..."
!endif

!define APP_NAME "Feishu VS Code Bridge"
!define COMPANY_NAME "Bean Harbor"
!define INSTALL_DIR "$LOCALAPPDATA\Programs\${APP_NAME}"
!define OUTPUT_EXE "${OUTPUT_DIR}\FeishuVSCodeBridgeSetup.exe"

Name "${APP_NAME}"
OutFile "${OUTPUT_EXE}"
InstallDir "${INSTALL_DIR}"
InstallDirRegKey HKCU "Software\${COMPANY_NAME}\${APP_NAME}" "InstallDir"
RequestExecutionLevel user

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\setup-gui.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch setup wizard after install"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"

Section "Install"
  SetOutPath "$INSTDIR"
  File "/oname=bridge-cli.exe" "${PAYLOAD_DIR}\bridge-cli.exe"
  File "/oname=setup-gui.exe" "${PAYLOAD_DIR}\setup-gui.exe"

  IfFileExists "${PAYLOAD_DIR}\feishu-agent-bridge.vsix" 0 +2
  File "/oname=feishu-agent-bridge.vsix" "${PAYLOAD_DIR}\feishu-agent-bridge.vsix"

  WriteRegStr HKCU "Software\${COMPANY_NAME}\${APP_NAME}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayVersion" "0.1.0"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "Publisher" "${COMPANY_NAME}"

  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\Setup Wizard.lnk" "$INSTDIR\setup-gui.exe"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\Uninstall ${APP_NAME}.lnk" "$INSTDIR\Uninstall.exe"
  CreateShortcut "$DESKTOP\${APP_NAME} Setup Wizard.lnk" "$INSTDIR\setup-gui.exe"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\bridge-cli.exe"
  Delete "$INSTDIR\setup-gui.exe"
  Delete "$INSTDIR\feishu-agent-bridge.vsix"
  Delete "$INSTDIR\Uninstall.exe"

  Delete "$SMPROGRAMS\${APP_NAME}\Setup Wizard.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\Uninstall ${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  Delete "$DESKTOP\${APP_NAME} Setup Wizard.lnk"

  DeleteRegKey HKCU "Software\${COMPANY_NAME}\${APP_NAME}"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
  RMDir "$INSTDIR"
SectionEnd