param(
    [Parameter(Mandatory)] [ValidateSet('up', 'start', 'stop', 'quit', 'status')] [string] $Action,
    [string] $Name
)

$dir = Join-Path $PSScriptRoot '..\work\captures' | Resolve-Path -ErrorAction SilentlyContinue
if (-not $dir) { $dir = (New-Item -ItemType Directory -Force (Join-Path $PSScriptRoot '..\work\captures')).FullName }
$ctl = Join-Path $dir 'ctl'
New-Item -ItemType Directory -Force $ctl | Out-Null

switch ($Action) {
    'up' {
        $worker = Join-Path $PSScriptRoot 'capture-worker.ps1'
        Start-Process -FilePath powershell.exe -Verb RunAs -WindowStyle Hidden `
            -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$worker`"", '-Dir', "`"$dir`"")
    }
    'start' {
        if (-not $Name) { throw 'start needs -Name' }
        New-Item -ItemType File -Force (Join-Path $ctl "start-$Name") | Out-Null
    }
    'stop' { New-Item -ItemType File -Force (Join-Path $ctl 'stop') | Out-Null }
    'quit' { New-Item -ItemType File -Force (Join-Path $ctl 'quit') | Out-Null }
    'status' {
        Get-Content -LiteralPath (Join-Path $ctl 'worker.log') -Tail 8 -ErrorAction SilentlyContinue
        Get-ChildItem -LiteralPath $dir -Filter *.pcap | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize | Out-String
    }
}
