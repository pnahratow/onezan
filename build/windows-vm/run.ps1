# Run by the scheduled task "onezan-qa" in the logged-in session. Follows qa\steps.txt:
#   start [VST3_PATH]   start onezan (killing a running one), with Mesa's llvmpipe
#   click X Y           left click at screen pixel X Y
#   keys TEXT           SendKeys syntax (e.g. " " for Space, "{ESC}")
#   sleep N             seconds
#   shot NAME           screenshot to qa\NAME.png
#   top                 move onezan's main window to the top left, in front
#   place X Y TITLE     move the window titled TITLE to X Y, in front
#   clickin DX DY TITLE click relative to the top left of the window titled TITLE
#   kill                stop onezan
$qa = "$HOME\qa"
$exe = (Get-ChildItem "$HOME\onezan\*\onezan.exe" | Select-Object -First 1).FullName
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class M {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
}
"@
$log = "$qa\steps.log"
"" | Out-File $log
foreach ($line in Get-Content "$qa\steps.txt") {
  $w = $line -split ' ', 2
  "$line" | Out-File -Append $log
  switch ($w[0]) {
    'start' {
      Get-Process onezan -ErrorAction SilentlyContinue | Stop-Process -Force
      $env:RUST_LOG = "info"; $env:GALLIUM_DRIVER = "llvmpipe"
      if ($w.Count -gt 1) { $env:VST3_PATH = $w[1] } else { Remove-Item Env:VST3_PATH -ErrorAction SilentlyContinue }
      Start-Process -FilePath $exe -RedirectStandardOutput "$qa\out.txt" -RedirectStandardError "$qa\err.txt"
    }
    'click' {
      $p = $w[1] -split ' '
      [M]::SetCursorPos([int]$p[0], [int]$p[1]); Start-Sleep -Milliseconds 150
      [M]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
      [M]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    }
    'keys' { [System.Windows.Forms.SendKeys]::SendWait($w[1]) }
    'sleep' { Start-Sleep -Seconds ([double]$w[1]) }
    'shot' {
      $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
      $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
      $g = [System.Drawing.Graphics]::FromImage($bmp)
      $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
      $bmp.Save("$qa\$($w[1]).png"); $g.Dispose(); $bmp.Dispose()
    }
    'top' {
      $h = (Get-Process onezan | Where-Object MainWindowHandle -ne 0 | Select-Object -First 1).MainWindowHandle
      [M]::SetWindowPos($h, [IntPtr]::Zero, 0, 0, 0, 0, 0x0001 -bor 0x0004) | Out-Null
      [M]::SetForegroundWindow($h) | Out-Null
    }
    'place' {
      # place X Y TITLE: moves the window with that exact title
      $p = $w[1] -split ' ', 3
      $h = [M]::FindWindow($null, $p[2])
      [M]::SetWindowPos($h, [IntPtr]::Zero, [int]$p[0], [int]$p[1], 0, 0, 0x0001 -bor 0x0004) | Out-Null
      [M]::SetForegroundWindow($h) | Out-Null
    }
    'clickin' {
      # clickin DX DY TITLE: clicks DX DY from the top left of the window titled TITLE
      $p = $w[1] -split ' ', 3
      $h = [M]::FindWindow($null, $p[2]); $r = New-Object M+R
      [M]::GetWindowRect($h, [ref]$r) | Out-Null
      "  window at $($r.L) $($r.T)" | Out-File -Append $log
      [M]::SetCursorPos($r.L + [int]$p[0], $r.T + [int]$p[1]); Start-Sleep -Milliseconds 150
      [M]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
      [M]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    }
    'kill' { Get-Process onezan -ErrorAction SilentlyContinue | Stop-Process -Force }
  }
}
(Get-Process onezan -ErrorAction SilentlyContinue | Measure-Object).Count | Out-File "$qa\running.txt"
"done" | Out-File "$qa\done.txt"
