param([string]$Zip)
Add-Type -AssemblyName System.IO.Compression.FileSystem
$z = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path $Zip))
$z.Entries | ForEach-Object { $_.FullName }
Write-Output ("TOTAL: " + $z.Entries.Count)
$z.Dispose()
