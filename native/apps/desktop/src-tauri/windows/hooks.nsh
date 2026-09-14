; O app importa d3d11.dll e mfplat.dll. O Direct3D 11 vem em todo Windows 10 e 11, e não
; há DirectX para instalar à parte: o redistribuível antigo só traz d3dx9 e xinput, que o app
; não usa. Já o Media Foundation, que é o encoder, falta nas edições N e KN, e sem ele o
; Windows nem abre o .exe. Ele é um recurso opcional do próprio Windows, e o instalador já
; roda como administrador por ser perMachine.
;
; O instalador é de 32 bits e o app só sai para 64: `Sysnative` é o System32 de verdade visto
; daqui, e o DISM de 32 bits se recusa a mexer num Windows de 64.
!macro NSIS_HOOK_PREINSTALL
  ${IfNot} ${FileExists} "$WINDIR\Sysnative\mfplat.dll"
    ${If} ${Cmd} `MessageBox MB_YESNO|MB_ICONEXCLAMATION "Este Windows está sem o Media Feature Pack, e o Unkvoid não abre sem ele.$\n$\nInstalar agora pelo Windows Update? Precisa de internet e pode levar alguns minutos." /SD IDNO IDYES`
      DetailPrint "Instalando o Media Feature Pack..."
      Push $0
      nsExec::ExecToLog `"$WINDIR\Sysnative\dism.exe" /Online /NoRestart /Add-Capability /CapabilityName:Media.MediaFeaturePack~~~~0.0.1.0`
      Pop $0

      ; 3010 é sucesso pedindo reinício; "error" e "timeout" chegam como texto.
      ${If} $0 == 3010
        MessageBox MB_OK|MB_ICONINFORMATION "Media Feature Pack instalado. Reinicie o computador antes de abrir o Unkvoid." /SD IDOK
      ${ElseIf} $0 != 0
        MessageBox MB_OK|MB_ICONSTOP "Não deu para instalar o Media Feature Pack (código $0). Instale em Configurações > Aplicativos > Recursos opcionais > Media Feature Pack." /SD IDOK
      ${EndIf}

      Pop $0
    ${EndIf}
  ${EndIf}
!macroend
