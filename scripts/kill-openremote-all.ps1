Get-Process openremote-ui-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
$ours = Get-CimInstance Win32_Process -Filter "name='msedgewebview2.exe'" |
  Where-Object { $_.CommandLine -match 'space\.moreweb\.openremote' }
foreach ($p in $ours) {
  Write-Output "killing webview PID $($p.ProcessId)"
  Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue
}
Start-Sleep -Seconds 3
$left = Get-CimInstance Win32_Process -Filter "name='msedgewebview2.exe'" |
  Where-Object { $_.CommandLine -match 'space\.moreweb\.openremote' }
Write-Output "remaining ours: $($left.Count)"
$port = Get-NetTCPConnection -LocalPort 9222 -State Listen -ErrorAction SilentlyContinue
Write-Output "port 9222 listeners: $($port.Count)"
