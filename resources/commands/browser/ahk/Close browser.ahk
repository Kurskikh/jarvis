; Rerun as admin, if required.
; Run the interpreter explicitly instead of the .ahk on its own: a bare script
; path goes through the shell association, which on many machines opens .ahk in
; an editor rather than running it.
If Not A_IsAdmin
{
    If (A_AhkPath != "")
        Run *RunAs "%A_AhkPath%" "%A_ScriptFullPath%"
    Else
        Run *RunAs "%A_ScriptFullPath%"
    ExitApp
}

; set partial title matching mode
SetTitleMatchMode, 2

; list of all browsers to close
GroupAdd, browsers, ahk_class MozillaWindowClass
GroupAdd, browsers, ahk_class IEFrame
GroupAdd, browsers, ahk_exe msedge.exe
GroupAdd, browsers, ahk_exe chrome.exe
GroupAdd, browsers, ahk_exe firefox.exe

; kill them all
Winclose, ahk_group browsers