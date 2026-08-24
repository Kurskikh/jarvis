# Yandex Music control for Jarvis.
#
# Transport goes through the app's own system media session
# (ru.yandex.desktop.music), not through media keys and not through window
# hotkeys. Both alternatives were tried and both are wrong here:
#
#   media keys  - Windows delivers them to whichever app it counts as the
#                 current media session. With a browser tab playing, "next
#                 track" skips the browser instead. Measured: a probe pressed
#                 play/pause and paused a Chrome tab that had nothing to do
#                 with music.
#   window keys - K, N, P and the rest are the app's own hotkeys, and they do
#                 not arrive at all. Not through AutoHotkey's ControlSend, not
#                 through PostMessage, not through keybd_event with the window
#                 verifiably in front. Proven with the W key, which toggles the
#                 fullscreen player and so moves the window when it lands: the
#                 geometry never changed once.
#
# Addressing the session by name has neither problem: precise, and the window
# stays where it is.
#
# Like, dislike, shuffle and repeat are absent on purpose. The session does not
# offer them - IsShuffleEnabled and IsRepeatEnabled both read False - and the
# window hotkeys that would do them never arrive. A command that answers "done"
# and changes nothing is worse than no command.
#
#   powershell -NoProfile -File ym.ps1 <verb>
#   verbs: toggle play pause next prev open close

param([Parameter(Mandatory = $true)][string]$Verb)

# A trace, because this runs detached and hidden: Jarvis launches the wrapper
# through AutoHotkey's UX launcher with /Launch, which hands off and exits, so
# nothing this script says reaches a console, an exit code, or the assistant.
# Without a file there is no way to tell "it ran and failed" from "it never
# ran" - and those need opposite fixes.
$LogPath = Join-Path $env:TEMP "jarvis_ym.log"
function Log($msg) {
    try {
        $line = "{0}  {1,-8} {2}" -f (Get-Date -Format "HH:mm:ss.fff"), $Verb, $msg
        Add-Content -Path $LogPath -Value $line -Encoding UTF8
    } catch { }
}
Log "--- start (pwsh $($PSVersionTable.PSVersion)) ---"
trap { Log "НЕОБРАБОТАННОЕ: $($_.Exception.GetType().Name): $($_.Exception.Message)"; continue }

# The app does not have one identity. Launched from the Start Menu shortcut it
# registers its media session as "ru.yandex.desktop.music"; launched by path -
# which is what this script does - Windows has no AppUserModelId to attach and
# falls back to the executable name, "Яндекс Музыка.exe".
#
# That is exactly why the transport stopped working after Jarvis opened the
# player itself: the session was there and playing, under a name the filter
# did not recognise. Match either, and anything else it may call itself later.
$AppIdPattern = 'yandex|яндекс'
$ExeName = 'Яндекс Музыка.exe'

# ------------------------------------------------------------ media session
function Get-Manager {
    Add-Type -AssemblyName System.Runtime.WindowsRuntime
    $script:AsTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
        $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
    $t = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager, Windows.Media.Control, ContentType=WindowsRuntime]
    Await ($t::RequestAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager])
}

function Await($op, $type) {
    $script:AsTask.MakeGenericMethod($type).Invoke($null, @($op)).GetAwaiter().GetResult()
}

function Get-Session([switch]$Quiet) {
    try { $mgr = Get-Manager } catch { Log "WinRT недоступен: $($_.Exception.Message)"; return $null }
    $s = $mgr.GetSessions() | Where-Object { $_.SourceAppUserModelId -match $AppIdPattern } | Select-Object -First 1
    if ($Quiet) { return $s }
    if ($s) {
        Log "сессия: $($s.SourceAppUserModelId)"
    } else {
        # name what IS there. A bare "not found" is what hid this bug: the
        # session existed and was playing the whole time, under another name.
        $names = ($mgr.GetSessions() | ForEach-Object { $_.SourceAppUserModelId }) -join ', '
        Log "сессии нет. Есть: $(if ($names) { $names } else { 'ни одной' })"
    }
    return $s
}

# ------------------------------------------------------------- window input
Add-Type @'
using System; using System.Runtime.InteropServices;
public class YmWin {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern void keybd_event(byte k, byte s, uint f, IntPtr e);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint from, uint to, bool attach);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();

  // Windows refuses SetForegroundWindow to a process that does not already own
  // the foreground or the last input event. This script is launched hidden and
  // detached, so it owns neither: the call returns and does nothing, and the
  // keystroke lands in whatever the person was actually looking at.
  //
  // Attaching our input queue to the current foreground window's thread makes
  // the system treat us as part of that thread for the length of the call,
  // which is the documented way through. Detached immediately afterwards -
  // leaving threads attached couples their input queues and can hang both.
  public static bool Focus(IntPtr target) {
    IntPtr fore = GetForegroundWindow();
    if (fore == target) return true;
    uint us = GetCurrentThreadId();
    uint theirs = GetWindowThreadProcessId(fore, IntPtr.Zero);
    bool attached = (theirs != 0 && theirs != us) && AttachThreadInput(us, theirs, true);
    try {
      ShowWindow(target, 9);          // SW_RESTORE
      BringWindowToTop(target);
      SetForegroundWindow(target);
    } finally {
      if (attached) AttachThreadInput(us, theirs, false);
    }
    return GetForegroundWindow() == target;
  }

  public struct RECT { public int L, T, R, B; }
  public struct POINT { public int X, Y; }
}
'@

# Ask for real pixels. Without this the process is scaled by Windows and
# GetWindowRect answers in logical units while the screen capture and the
# cursor work in physical ones - they agree only at 100% scaling. An earlier
# click test missed the window entirely for exactly this reason, and the
# "clicks do not work either" conclusion drawn from it was wrong.
[YmWin]::SetProcessDPIAware() | Out-Null

function Get-MainWindow {
    (Get-Process -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -match 'Яндекс Музыка' } |
        Select-Object -First 1).MainWindowHandle
}

# ------------------------------------------------------------ starting play
# Starting from cold is the one thing the media session cannot do: it does not
# exist until the first note, so there is nothing to command. The app's own
# hotkeys are no help either - see the note at the top; they never arrive.
#
# A synthesised CLICK does arrive. That is the whole asymmetry, and it took a
# while to see because the one click test that would have shown it was itself
# broken - it used logical coordinates against a physical screen and landed
# outside the window, which was then read as "clicks do not work either".
#
# So: find the play button and click it, where a person would.
#
# The button is located, not assumed. The window is captured and the centre of
# mass of the yellow pixels in the transport strip is taken. While nothing is
# playing that button is a solid yellow disc and by far the largest yellow
# thing down there; the previous and next chevrons sit symmetrically either
# side, so they pull the centre sideways by nothing. Checked against a real
# capture, the estimate landed one pixel off the button's centre.
#
# It costs a moment of stolen focus, since a window has to be in front to be
# clicked. The previous foreground window and the cursor are put back, and
# none of this runs when there is a session to command.

function Find-PlayButton([IntPtr]$h) {
    Add-Type -AssemblyName System.Drawing
    $r = New-Object YmWin+RECT
    if (-not [YmWin]::GetWindowRect($h, [ref]$r)) { return $null }
    $w = $r.R - $r.L; $ht = $r.B - $r.T
    if ($w -lt 400 -or $ht -lt 300) { Log "окно слишком мало: $w x $ht"; return $null }

    $full = New-Object System.Drawing.Bitmap($w, $ht)
    $g = [System.Drawing.Graphics]::FromImage($full)
    $g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object System.Drawing.Size($w, $ht)))
    $g.Dispose()
    # half size: four times fewer pixels to walk, disc still ~56 across
    $sw = [int]($w / 2); $sh = [int]($ht / 2)
    $img = New-Object System.Drawing.Bitmap($full, (New-Object System.Drawing.Size($sw, $sh)))
    $full.Dispose()

    $x0 = [int]($sw * 0.50); $x1 = [int]($sw * 0.95)
    $y0 = [int]($sh * 0.72); $y1 = [int]($sh * 0.88)
    $sx = 0; $sy = 0; $n = 0
    for ($y = $y0; $y -lt $y1; $y++) {
        for ($x = $x0; $x -lt $x1; $x++) {
            $c = $img.GetPixel($x, $y)
            if ($c.R -gt 200 -and $c.G -gt 150 -and $c.B -lt 130 -and ($c.R - $c.B) -gt 100) {
                $sx += $x; $sy += $y; $n++
            }
        }
    }
    $img.Dispose()
    # a handful of stray yellow pixels is not a button
    if ($n -lt 300) { Log "жёлтого в полосе управления мало ($n точек) - кнопку не опознал"; return $null }
    [pscustomobject]@{
        X = $r.L + [int]($sx / $n) * 2
        Y = $r.T + [int]($sy / $n) * 2
        N = $n
    }
}

function Start-Playback {
    $h = Get-MainWindow
    if (-not $h) {
        if (-not (Start-App)) { Log "исполняемый файл не найден"; return $false }
        Start-Sleep -Seconds 5   # the page has to draw before there is a button to find
        $h = Get-MainWindow
        if (-not $h) { Log "окно так и не появилось"; return $false }
    }

    $prev = [YmWin]::GetForegroundWindow()
    $save = New-Object YmWin+POINT
    [YmWin]::GetCursorPos([ref]$save) | Out-Null

    if (-not [YmWin]::Focus($h)) { Log "окно не удалось поднять"; return $false }
    Start-Sleep -Milliseconds 700

    $pt = Find-PlayButton $h
    if (-not $pt) {
        if ($prev -ne [IntPtr]::Zero -and $prev -ne $h) { [YmWin]::Focus($prev) | Out-Null }
        return $false
    }
    Log "кнопка play: $($pt.X),$($pt.Y) (жёлтых точек $($pt.N))"

    [YmWin]::SetCursorPos($pt.X, $pt.Y) | Out-Null
    Start-Sleep -Milliseconds 250
    [YmWin]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [YmWin]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)

    # put the desk back before waiting, so the pause is not felt as a freeze
    [YmWin]::SetCursorPos($save.X, $save.Y) | Out-Null
    if ($prev -ne [IntPtr]::Zero -and $prev -ne $h) { [YmWin]::Focus($prev) | Out-Null }

    # verify rather than assume: the session appearing is the proof it played
    for ($i = 0; $i -lt 12; $i++) {
        Start-Sleep -Milliseconds 500
        if (Get-Session -Quiet) { Log "заиграло"; return $true }
    }
    Log "клик прошёл, но воспроизведение не началось"
    return $false
}


function Start-App {
    $exe = Join-Path $env:LOCALAPPDATA "Programs\YandexMusic\$ExeName"
    if (-not (Test-Path $exe)) { return $false }
    Start-Process $exe
    # the session does not exist until the first note plays, so a caller that
    # wants to play has to wait for the window before it can press anything
    for ($i = 0; $i -lt 30; $i++) {
        Start-Sleep -Milliseconds 500
        if (Get-MainWindow) { return $true }
    }
    return $true
}

# ------------------------------------------------------------------- verbs
switch ($Verb.ToLower()) {

    { $_ -in 'toggle', 'play', 'pause' } {
        $s = Get-Session
        if ($s) {
            switch ($Verb.ToLower()) {
                'play'  { Await ($s.TryPlayAsync()) ([bool]) | Out-Null }
                'pause' { Await ($s.TryPauseAsync()) ([bool]) | Out-Null }
                default { $r = Await ($s.TryTogglePlayPauseAsync()) ([bool]); Log "TryToggle=$r" }
            }
            break
        }
        # No session: nothing is playing. "Pause" is then already satisfied;
        # anything else means start it.
        if ($Verb.ToLower() -eq 'pause') { Log "и так ничего не играет"; break }
        if (Start-Playback) { Log "воспроизведение запущено" }
    }

    'next' {
        $s = Get-Session
        if (-not $s -and (Start-Playback)) { $s = Get-Session -Quiet }
        if ($s) { $r = Await ($s.TrySkipNextAsync()) ([bool]); Log "TrySkipNext=$r" }
        else { Log "переключать нечего - запустить воспроизведение не вышло" }
    }

    'prev' {
        $s = Get-Session
        if (-not $s -and (Start-Playback)) { $s = Get-Session -Quiet }
        if ($s) { $r = Await ($s.TrySkipPreviousAsync()) ([bool]); Log "TrySkipPrev=$r" }
        else { Log "переключать нечего - запустить воспроизведение не вышло" }
    }

    'open' {
        if (-not (Get-MainWindow)) {
            if (-not (Start-App)) { Log "исполняемый файл не найден"; exit 1 }
        }
        $h = Get-MainWindow
        if ($h) { [YmWin]::Focus($h) | Out-Null }
        if (Get-Session) { Log "уже играет, окно поднято" }
        else { Log "окно открыто, ничего не играет" }
    }

    'close' {
        # ask nicely first: a clean exit keeps the queue and the position
        $p = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowTitle -match 'Яндекс Музыка' }
        if ($p) { $p.CloseMainWindow() | Out-Null; Start-Sleep -Milliseconds 800 }
        Get-Process -ErrorAction SilentlyContinue |
            Where-Object { $_.ProcessName -match '^Яндекс Музыка$' } |
            ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
    }

    default { exit 2 }
}

Log "--- done ---"
