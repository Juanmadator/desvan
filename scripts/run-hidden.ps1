# Lanza un programa en un escritorio de Windows oculto (no aparece en pantalla ni en la barra de tareas).
# Uso: powershell -File scripts/run-hidden.ps1 -Exe ruta.exe [-Arguments "..."] [-DebugPort 9333]
# Con -DebugPort, WebView2 abre el protocolo de depuración para conectar Playwright (connectOverCDP).
param(
  [Parameter(Mandatory = $true)][string]$Exe,
  [string]$Arguments = "",
  [int]$DebugPort = 0,
  [string]$Desktop = "desvan-oculto"
)

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class Oculto {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct STARTUPINFO {
    public int cb; public string lpReserved; public string lpDesktop; public string lpTitle;
    public int dwX; public int dwY; public int dwXSize; public int dwYSize; public int dwXCountChars;
    public int dwYCountChars; public int dwFillAttribute; public int dwFlags; public short wShowWindow;
    public short cbReserved2; public IntPtr lpReserved2; public IntPtr hStdInput; public IntPtr hStdOutput; public IntPtr hStdError;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct PROCESS_INFORMATION { public IntPtr hProcess; public IntPtr hThread; public int dwProcessId; public int dwThreadId; }
  [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern IntPtr CreateDesktopW(string name, IntPtr dev, IntPtr mode, int flags, uint access, IntPtr sa);
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern bool CreateProcessW(string app, string cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string dir, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
  public static int Run(string desktop, string exe, string args) {
    IntPtr d = CreateDesktopW(desktop, IntPtr.Zero, IntPtr.Zero, 0, 0x10000000u, IntPtr.Zero);
    if (d == IntPtr.Zero) throw new Exception("CreateDesktop: " + Marshal.GetLastWin32Error());
    var si = new STARTUPINFO(); si.cb = Marshal.SizeOf(si); si.lpDesktop = desktop;
    PROCESS_INFORMATION pi;
    string cmd = "\"" + exe + "\" " + args;
    if (!CreateProcessW(null, cmd, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, System.IO.Path.GetDirectoryName(exe), ref si, out pi))
      throw new Exception("CreateProcess: " + Marshal.GetLastWin32Error());
    return pi.dwProcessId;
  }
}
"@

if ($DebugPort -gt 0) {
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort"
}
$procId = [Oculto]::Run($Desktop, (Resolve-Path $Exe).Path, $Arguments)
Write-Output $procId
