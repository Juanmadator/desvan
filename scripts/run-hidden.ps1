# Lanza un programa en un escritorio de Windows oculto (no aparece en pantalla ni en la barra de tareas).
# El lanzador se queda vivo mientras el programa corre: si se cerrara, el escritorio se destruiría
# y el programa no podría arrancar. Escribe el PID en la primera línea de la salida.
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
  [DllImport("kernel32.dll")]
  public static extern uint WaitForSingleObject(IntPtr h, uint ms);
  public static IntPtr Desk;
  public static PROCESS_INFORMATION Start(string desktop, string exe, string args) {
    Desk = CreateDesktopW(desktop, IntPtr.Zero, IntPtr.Zero, 0, 0x10000000u, IntPtr.Zero);
    if (Desk == IntPtr.Zero) throw new Exception("CreateDesktop: " + Marshal.GetLastWin32Error());
    var si = new STARTUPINFO(); si.cb = Marshal.SizeOf(si); si.lpDesktop = desktop;
    PROCESS_INFORMATION pi;
    string cmd = "\"" + exe + "\" " + args;
    // 0x10 = CREATE_NEW_CONSOLE: la consola (si la hay) también queda en el escritorio oculto
    if (!CreateProcessW(null, cmd, IntPtr.Zero, IntPtr.Zero, false, 0x10, IntPtr.Zero, System.IO.Path.GetDirectoryName(exe), ref si, out pi))
      throw new Exception("CreateProcess: " + Marshal.GetLastWin32Error());
    return pi;
  }
}
"@

if ($DebugPort -gt 0) {
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort"
  # carpeta de datos propia: si la app instalada está abierta, WebView2 reutilizaría su proceso
  $env:DESVAN_WEBVIEW_DATA = Join-Path $env:TEMP "desvan-webview-pruebas-$DebugPort"
}
$pi = [Oculto]::Start($Desktop, (Resolve-Path $Exe).Path, $Arguments)
[Console]::Out.WriteLine($pi.dwProcessId)
[Console]::Out.Flush()
# mantiene vivo el escritorio hasta que el programa termine
[void][Oculto]::WaitForSingleObject($pi.hProcess, [uint32]::MaxValue)
