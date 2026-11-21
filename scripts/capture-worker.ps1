param(
    [Parameter(Mandatory)] [string] $Dir,
    [string] $Hub = '\\.\USBPcap5',
    [string] $Device = '1'
)

$ErrorActionPreference = 'Stop'
$exe = 'C:\Program Files\USBPcap\USBPcapCMD.exe'
$ctl = Join-Path $Dir 'ctl'
$log = Join-Path $ctl 'worker.log'
New-Item -ItemType Directory -Force $ctl | Out-Null
$current = $null

function Write-Log([string] $msg) {
    Add-Content -LiteralPath $log -Value ("{0:HH:mm:ss.fff} {1}" -f (Get-Date), $msg) -Encoding utf8
}

function Stop-Capture {
    if ($script:current) {
        Stop-Process -Id $script:current.Id -Force -ErrorAction SilentlyContinue
        $script:current.WaitForExit()
        Write-Log "stopped pid $($script:current.Id)"
        $script:current = $null
    }
}

Write-Log "worker up: hub $Hub device $Device"
while ($true) {
    $flags = Get-ChildItem -LiteralPath $ctl -File | Where-Object { $_.Name -like 'start-*' -or $_.Name -eq 'stop' -or $_.Name -eq 'quit' } | Sort-Object LastWriteTime
    foreach ($f in $flags) {
        Remove-Item -LiteralPath $f.FullName -Force
        Stop-Capture
        switch -Wildcard ($f.Name) {
            'start-*' {
                $name = $f.Name.Substring(6)
                $out = Join-Path $Dir "$name.pcap"
                $script:current = Start-Process -FilePath $exe -ArgumentList @('-d', $Hub, '--devices', $Device, '-o', "`"$out`"") -WindowStyle Hidden -PassThru
                Write-Log "started $name pid $($script:current.Id) -> $out"
            }
            'quit' { Write-Log 'quit'; exit 0 }
        }
    }
    if ($script:current -and $script:current.HasExited) {
        Write-Log "capture pid $($script:current.Id) exited by itself (code $($script:current.ExitCode))"
        $script:current = $null
    }
    Start-Sleep -Milliseconds 200
}
