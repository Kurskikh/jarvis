#NoEnv
SetWorkingDir %A_ScriptDir%

; Hide, so no console flashes on a spoken command. The real work is in
; ym.ps1, one directory up - see the note at the top of it for why the
; transport goes through the media session rather than through key presses.
Run, powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%A_ScriptDir%\..\ym.ps1" close, , Hide
