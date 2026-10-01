$procs = Get-CimInstance Win32_Process -Filter "name='msedgewebview2.exe'"
$ours = $procs | Where-Object { $_.CommandLine -match 'openremote|moreweb' }
if (-not $ours) {
  Write-Output "NO openremote webview processes found among $($procs.Count) total"
  $procs | ForEach-Object { $ud = if ($_.CommandLine -match 'user-data-dir=([^ ]+)') { $Matches[1] } else { '?' }; "$($_.ProcessId)  $ud" }
} else {
  foreach ($p in $ours) {
    Write-Output "=== PID $($p.ProcessId) ==="
    $p.CommandLine -split ' --' | Select-Object -First 14
  }
}
