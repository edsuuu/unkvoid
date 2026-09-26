; O instalador do app nativo do Windows.
;
; Ele ocupa o lugar exato do app do Tauri — a mesma pasta, o mesmo `unkvoid-desktop.exe`, a
; mesma chave de desinstalação —, e é isso que migra quem ainda tem o Tauri: o atualizador
; dele baixa este instalador, confere a assinatura com a mesma chave e o abre com
; `/P /UPDATE /R /ARGS …`. O mesmo nome de arquivo mantém o atalho do menu Iniciar e o app
; fixado na barra de tarefas apontando para o lugar certo.
;
; Só há a página do progresso: sem perguntas, instalar é o `/P` (passivo) do Tauri o tempo
; todo. `/S` é o silencioso do próprio NSIS.
;
;   makensis /DVERSION=0.1.0-beta /DBINARY=…\unkvoid.exe /DICON=…\icon.ico /DOUTFILE=…\setup.exe installer.nsi

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma

!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"

!define PRODUCT "Unkvoid"
!define EXE "unkvoid-desktop.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT}"

Name "${PRODUCT}"
OutFile "${OUTFILE}"
Icon "${ICON}"
UninstallIcon "${ICON}"
InstallDir "$PROGRAMFILES64\${PRODUCT}"
RequestExecutionLevel admin
AutoCloseWindow true
ShowInstDetails nevershow
ShowUninstDetails nevershow
BrandingText "${PRODUCT} ${VERSION}"

Page instfiles
UninstPage instfiles

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "O Unkvoid só roda no Windows de 64 bits." /SD IDOK
    Abort
  ${EndIf}

  SetRegView 64
  SetShellVarContext all
FunctionEnd

Function un.onInit
  SetRegView 64
  SetShellVarContext all
FunctionEnd

; O app importa o Media Foundation, que é o encoder, e ele falta nas edições N e KN — sem ele
; o Windows nem abre o .exe. O instalador é de 32 bits: `Sysnative` é o System32 de verdade
; visto daqui, e o DISM de 32 bits se recusa a mexer num Windows de 64.
Function MediaFeaturePack
  ${If} ${FileExists} "$WINDIR\Sysnative\mfplat.dll"
    Return
  ${EndIf}

  ${If} ${Cmd} `MessageBox MB_YESNO|MB_ICONEXCLAMATION "Este Windows está sem o Media Feature Pack, e o Unkvoid não abre sem ele.$\n$\nInstalar agora pelo Windows Update? Precisa de internet e pode levar alguns minutos." /SD IDNO IDYES`
    DetailPrint "Instalando o Media Feature Pack..."
    nsExec::ExecToLog `"$WINDIR\Sysnative\dism.exe" /Online /NoRestart /Add-Capability /CapabilityName:Media.MediaFeaturePack~~~~0.0.1.0`
    Pop $0

    ; 3010 é sucesso pedindo reinício; "error" e "timeout" chegam como texto.
    ${If} $0 == 3010
      MessageBox MB_OK|MB_ICONINFORMATION "Media Feature Pack instalado. Reinicie o computador antes de abrir o Unkvoid." /SD IDOK
    ${ElseIf} $0 != 0
      MessageBox MB_OK|MB_ICONSTOP "Não deu para instalar o Media Feature Pack (código $0). Instale em Configurações > Aplicativos > Recursos opcionais > Media Feature Pack." /SD IDOK
    ${EndIf}
  ${EndIf}
FunctionEnd

Section
  Call MediaFeaturePack

  ; O app aberto trava o .exe. O atualizador sai sozinho logo depois de abrir o instalador,
  ; mas não espera por nós; quem abriu à mão também está aqui.
  nsExec::Exec `taskkill /F /T /IM ${EXE}`
  Pop $0
  Sleep 800

  SetOutPath "$INSTDIR"
  File "/oname=${EXE}" "${BINARY}"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateShortcut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${EXE}"

  ; Na atualização o atalho da área de trabalho fica como a pessoa deixou: quem o apagou não
  ; quer vê-lo de volta a cada versão.
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/UPDATE" $R1
  ${If} ${Errors}
    CreateShortcut "$DESKTOP\${PRODUCT}.lnk" "$INSTDIR\${EXE}"
  ${EndIf}

  WriteRegStr HKLM "Software\unkvoid\${PRODUCT}" "" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayIcon" '"$INSTDIR\${EXE}"'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "Publisher" "${PRODUCT}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "MainBinaryName" "${EXE}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "InstallLocation" '"$INSTDIR"'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "URLInfoAbout" "https://unkvoid.com"
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoRepair" 1

  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "EstimatedSize" "$0"

  ; Abre o app no fim — menos no silencioso sem `/R`, que é instalação por script. Pelo
  ; Explorer, e não direto: daqui ele herdaria o administrador do instalador.
  ClearErrors
  ${GetOptions} $R0 "/R" $R1
  ${IfNot} ${Errors}
  ${OrIfNot} ${Silent}
    Exec `"$WINDIR\explorer.exe" "$INSTDIR\${EXE}"`
  ${EndIf}
SectionEnd

Section "Uninstall"
  nsExec::Exec `taskkill /F /T /IM ${EXE}`
  Pop $0
  Sleep 800

  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  Delete "$DESKTOP\${PRODUCT}.lnk"

  DeleteRegKey HKLM "${UNINSTALL_KEY}"
  DeleteRegKey HKLM "Software\unkvoid\${PRODUCT}"
  DeleteRegKey /ifempty HKLM "Software\unkvoid"
SectionEnd
